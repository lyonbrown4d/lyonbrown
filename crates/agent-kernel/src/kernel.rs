use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_context::{CompileRequest, PromptCompiler, TokenEstimator};
use agent_core::{
    AgentError, AgentEvent, ContextItem, ContextItemKind, ObservationId, StepId, StepOutcome,
    StopReason, TokenUsage, ToolResult, TurnId,
};
use agent_model::{
    ChatMessage, GateVerdict, HeuristicGate, ModelEvent, ModelRegistry, ModelRequest,
    ModelScheduler, QualityGate, Role, RoutingPolicy, StepContext,
};
use agent_profile::AgentProfile;
use agent_secret::{NoopBroker, SecretBroker};
use agent_skill::{Skill, SkillRegistry};
use agent_tool::{
    AllowAllApproval, ApprovalDecision, ApprovalPolicy, ToolContext, ToolExecutionError,
    ToolExecutor, ToolRegistry,
};
use agent_workspace::{ProjectType, WorkspaceProfile};
use chrono::Utc;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::{EventBus, KernelState, Session};

#[derive(Debug, Clone)]
pub struct KernelLimits {
    pub max_steps_per_turn: usize,
    pub max_escalations: usize,
    pub tool_timeout: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct KernelModelOptions {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

impl Default for KernelLimits {
    fn default() -> Self {
        Self {
            max_steps_per_turn: 20,
            max_escalations: 1,
            tool_timeout: Duration::from_secs(60),
        }
    }
}

pub struct Kernel {
    session: Session,
    state: KernelState,
    bus: EventBus,
    scheduler: ModelScheduler,
    tools: ToolRegistry,
    approval: Arc<dyn ApprovalPolicy>,
    broker: Arc<dyn SecretBroker>,
    gate: Arc<dyn QualityGate>,
    limits: KernelLimits,
    cancellation: CancellationToken,
    tool_context: ToolContext,
    model_options: KernelModelOptions,
    agent_profile: AgentProfile,
    workspace_profile: WorkspaceProfile,
    skills: SkillRegistry,
    prompt_compiler: PromptCompiler,
    last_requests: Vec<ModelRequest>,
}

impl Kernel {
    pub fn builder() -> KernelBuilder {
        KernelBuilder::default()
    }

    pub fn state(&self) -> &KernelState {
        &self.state
    }

    pub fn event_log(&self) -> &[AgentEvent] {
        self.bus.log()
    }

    pub fn last_requests(&self) -> &[ModelRequest] {
        &self.last_requests
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentEvent> {
        self.bus.subscribe()
    }

    pub async fn run_turn(&mut self, user_text: &str) -> Result<Vec<AgentEvent>, AgentError> {
        let log_start = self.bus.log().len();
        if !matches!(self.state, KernelState::Idle) {
            return Err(AgentError::Transition(format!(
                "cannot start turn from {:?}",
                self.state
            )));
        }

        let now = Utc::now();
        if !self.session.started {
            self.emit(AgentEvent::SessionStarted {
                session_id: self.session.id,
                ts: now,
            })?;
            self.session.started = true;
        }

        let turn_id = TurnId::new();
        self.emit(AgentEvent::UserMessageReceived {
            session_id: self.session.id,
            turn_id,
            text: user_text.to_owned(),
            ts: Utc::now(),
        })?;
        self.emit(AgentEvent::TurnStarted {
            session_id: self.session.id,
            turn_id,
            ts: Utc::now(),
        })?;
        self.session.push_message(
            ChatMessage {
                role: Role::User,
                content: user_text.to_owned(),
                tool_call_id: None,
                tool_name: None,
                tool_calls: vec![],
            },
            ContextItemKind::Message,
            "user",
        );

        let mut steps = 0;
        let mut escalations = 0;
        let mut used_tool = false;
        loop {
            if self.cancellation.is_cancelled() {
                self.cancel_turn(turn_id)?;
                return Err(AgentError::Cancelled);
            }
            if steps >= self.limits.max_steps_per_turn {
                let message = format!(
                    "max steps per turn exceeded ({})",
                    self.limits.max_steps_per_turn
                );
                self.emit(AgentEvent::Error {
                    session_id: self.session.id,
                    message: message.clone(),
                    ts: Utc::now(),
                })?;
                return Err(AgentError::LimitExceeded(message));
            }
            steps += 1;

            let step_id = StepId::new();
            self.emit(AgentEvent::StepStarted {
                session_id: self.session.id,
                turn_id,
                step_id,
                ts: Utc::now(),
            })?;
            let tool_definitions = self.tools.definitions();
            let mut step_context = StepContext::new(user_text);
            step_context.needs_tools =
                !tool_definitions.is_empty() && (used_tool || step_needs_tools(user_text));
            step_context.previous_steps = steps - 1;
            step_context.escalated = escalations > 0;
            step_context.provider = self.model_options.provider.clone();
            step_context.model = self.model_options.model.clone();
            let cancellation = self.cancellation.clone();
            let selected = match tokio::select! {
                () = cancellation.cancelled() => {
                    self.cancel_turn(turn_id)?;
                    return Err(AgentError::Cancelled);
                }
                selected = self.scheduler.decide(&step_context) => selected,
            } {
                Ok(selected) => selected,
                Err(error) => {
                    let error = AgentError::Model(error.to_string());
                    self.record_error(error.to_string())?;
                    return Err(error);
                }
            };
            let max_tokens = self
                .model_options
                .max_tokens
                .unwrap_or(selected.capabilities.max_output_tokens)
                .min(selected.capabilities.max_output_tokens);
            let input_budget = u64::from(
                selected
                    .capabilities
                    .context_window
                    .saturating_sub(max_tokens),
            );
            let request_tools = if step_context.needs_tools && selected.capabilities.supports_tools
            {
                tool_definitions
            } else {
                Vec::new()
            };
            let estimator = TokenEstimator::default();
            let fixed_context_tokens = serialized_context_tokens(&estimator, &[], &request_tools)?;
            let token_budget = match input_budget.checked_sub(fixed_context_tokens) {
                Some(budget) => budget,
                None => {
                    let error = AgentError::Context(format!(
                        "tool definitions require {fixed_context_tokens} tokens but input budget is {input_budget}"
                    ));
                    self.record_error(error.to_string())?;
                    return Err(error);
                }
            };
            let active_skills = match self.active_skills(user_text) {
                Ok(skills) => skills,
                Err(error) => {
                    let error = AgentError::Context(error);
                    self.record_error(error.to_string())?;
                    return Err(error);
                }
            };
            let budget_items = self.session.budget_items();
            let compiled = match self.prompt_compiler.compile(CompileRequest {
                profile: &self.agent_profile,
                model: &selected.capabilities,
                workspace: &self.workspace_profile,
                skills: &active_skills,
                tool_guidance: None,
                context_items: &budget_items,
                token_budget,
            }) {
                Ok(compiled) => compiled,
                Err(error) => {
                    let error = AgentError::Context(error.to_string());
                    self.record_error(error.to_string())?;
                    return Err(error);
                }
            };
            let request_messages = self.session.selected_messages(&compiled.context_items);
            let system_tokens = serialized_system_tokens(&estimator, &compiled.system_prompt)?;
            let context_tokens =
                serialized_context_tokens(&estimator, &request_messages, &request_tools)?;
            let finalized_tokens = system_tokens.checked_add(context_tokens).ok_or_else(|| {
                AgentError::Context("finalized request token count overflowed u64".into())
            })?;
            if finalized_tokens > input_budget {
                let error = AgentError::Context(format!(
                    "finalized request requires {finalized_tokens} tokens but input budget is {input_budget}"
                ));
                self.record_error(error.to_string())?;
                return Err(error);
            }
            let system_tokens = u32::try_from(system_tokens).map_err(|_| {
                AgentError::Context("compiled system token count exceeds u32".into())
            })?;
            let context_tokens = u32::try_from(context_tokens).map_err(|_| {
                AgentError::Context("compiled context token count exceeds u32".into())
            })?;
            let dropped = u32::try_from(compiled.dropped)
                .map_err(|_| AgentError::Context("dropped context count exceeds u32".into()))?;
            self.emit(AgentEvent::ContextPrepared {
                step_id,
                system_tokens,
                context_tokens,
                dropped,
                ts: Utc::now(),
            })?;
            self.emit(AgentEvent::ModelSelected {
                step_id,
                provider: selected.decision.provider.clone(),
                model: selected.capabilities.model.clone(),
                reason: selected.decision.reason.clone(),
                ts: Utc::now(),
            })?;

            let request = ModelRequest {
                model: selected.capabilities.model.clone(),
                system: Some(compiled.system_prompt),
                messages: request_messages,
                tools: request_tools,
                max_tokens,
                temperature: self.model_options.temperature,
                json_mode: false,
                thinking: None,
                stop: vec![],
            };
            self.last_requests.push(request.clone());
            self.emit(AgentEvent::ModelStarted {
                step_id,
                ts: Utc::now(),
            })?;

            let provider = selected.provider.clone();
            let cancellation = self.cancellation.clone();
            let mut stream = match tokio::select! {
                () = cancellation.cancelled() => {
                    self.cancel_turn(turn_id)?;
                    return Err(AgentError::Cancelled);
                }
                stream = provider.stream(request) => stream,
            } {
                Ok(stream) => stream.events,
                Err(error) => {
                    let error = AgentError::Model(error.to_string());
                    self.record_error(error.to_string())?;
                    return Err(error);
                }
            };
            let mut text = String::new();
            let mut tool_calls = Vec::new();
            let mut usage = TokenUsage::default();
            let mut stop_reason = None;
            let cancellation = self.cancellation.clone();
            loop {
                let event = tokio::select! {
                    () = cancellation.cancelled() => {
                        self.cancel_turn(turn_id)?;
                        return Err(AgentError::Cancelled);
                    }
                    event = stream.next() => event,
                };
                let Some(event) = event else { break };
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        let error = AgentError::Model(error.to_string());
                        self.record_error(error.to_string())?;
                        return Err(error);
                    }
                };
                match event {
                    ModelEvent::Delta(delta) => {
                        text.push_str(&delta);
                        self.emit(AgentEvent::ModelDelta { step_id, delta })?;
                    }
                    ModelEvent::ToolCall(invocation) => tool_calls.push(invocation),
                    ModelEvent::Usage(value) => usage = value,
                    ModelEvent::Done {
                        stop_reason: reason,
                    } => stop_reason = Some(reason),
                }
            }
            let stop_reason = match stop_reason {
                Some(stop_reason) => stop_reason,
                None => {
                    let error =
                        AgentError::Model("provider stream ended without a Done event".into());
                    self.record_error(error.to_string())?;
                    return Err(error);
                }
            };
            self.emit(AgentEvent::ModelCompleted {
                step_id,
                usage,
                ts: Utc::now(),
            })?;

            if tool_calls.is_empty() && matches!(stop_reason, StopReason::ToolUse) {
                let error =
                    AgentError::Model("provider returned ToolUse without any tool calls".into());
                self.record_error(error.to_string())?;
                return Err(error);
            }
            if !tool_calls.is_empty() {
                self.run_tools(turn_id, step_id, text, tool_calls).await?;
                used_tool = true;
                continue;
            }

            let mut outcome = StepOutcome {
                stop_reason,
                text: text.clone(),
                tool_calls: vec![],
                escalated: false,
            };
            match self.gate.evaluate(&outcome, &selected.capabilities) {
                GateVerdict::Pass => {
                    self.session.push_message(
                        ChatMessage {
                            role: Role::Assistant,
                            content: text.clone(),
                            tool_call_id: None,
                            tool_name: None,
                            tool_calls: vec![],
                        },
                        ContextItemKind::Message,
                        "assistant",
                    );
                    self.emit(AgentEvent::StepCompleted {
                        step_id,
                        outcome,
                        ts: Utc::now(),
                    })?;
                    self.emit(AgentEvent::TurnCompleted {
                        turn_id,
                        answer: text,
                        ts: Utc::now(),
                    })?;
                    return Ok(self.bus.log()[log_start..].to_vec());
                }
                GateVerdict::Escalate(_reason) if escalations < self.limits.max_escalations => {
                    escalations += 1;
                    outcome.escalated = true;
                    self.emit(AgentEvent::StepCompleted {
                        step_id,
                        outcome,
                        ts: Utc::now(),
                    })?;
                }
                GateVerdict::Escalate(reason) => {
                    self.emit(AgentEvent::Error {
                        session_id: self.session.id,
                        message: reason.clone(),
                        ts: Utc::now(),
                    })?;
                    return Err(AgentError::LimitExceeded(reason));
                }
            }
        }
    }

    async fn run_tools(
        &mut self,
        turn_id: TurnId,
        step_id: StepId,
        text: String,
        tool_calls: Vec<agent_core::ToolInvocation>,
    ) -> Result<(), AgentError> {
        let assistant_message = ChatMessage {
            role: Role::Assistant,
            content: text.clone(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: tool_calls.clone(),
        };
        self.session
            .push_unbudgeted_message(assistant_message.clone());

        let call_ids = tool_calls.iter().map(|call| call.id).collect();
        for invocation in tool_calls {
            self.emit(AgentEvent::ToolRequested {
                step_id,
                invocation: invocation.clone(),
                ts: Utc::now(),
            })?;
            let mut result = match self.tools.get(&invocation.name) {
                None => ToolResult {
                    invocation_id: invocation.id,
                    ok: false,
                    output: String::new(),
                    truncated: false,
                    duration_ms: 0,
                    error: Some(format!("tool not found: {}", invocation.name)),
                },
                Some(tool) => match self.approval.approve(tool.definition(), &invocation) {
                    ApprovalDecision::Deny | ApprovalDecision::DenyAll => ToolResult {
                        invocation_id: invocation.id,
                        ok: false,
                        output: String::new(),
                        truncated: false,
                        duration_ms: 0,
                        error: Some("tool denied by approval policy".into()),
                    },
                    ApprovalDecision::Allow | ApprovalDecision::AllowSession => {
                        self.emit(AgentEvent::ToolApproved {
                            step_id,
                            tool_call_id: invocation.id,
                            by: "policy".into(),
                            ts: Utc::now(),
                        })?;
                        self.emit(AgentEvent::ToolStarted {
                            step_id,
                            tool_call_id: invocation.id,
                            ts: Utc::now(),
                        })?;
                        let executor =
                            ToolExecutor::new(self.tools.clone(), self.tool_context.clone());
                        match executor.execute(&invocation).await {
                            Ok(result) => result,
                            Err(ToolExecutionError::Cancelled) => {
                                self.cancel_turn(turn_id)?;
                                return Err(AgentError::Cancelled);
                            }
                            Err(error) => ToolResult {
                                invocation_id: invocation.id,
                                ok: false,
                                output: String::new(),
                                truncated: false,
                                duration_ms: 0,
                                error: Some(error.to_string()),
                            },
                        }
                    }
                },
            };
            result.output = self.broker.redact(&result.output);
            result.error = result.error.map(|error| self.broker.redact(&error));
            let observation = if result.ok {
                result.output.clone()
            } else {
                serde_json::json!({
                    "ok": false,
                    "output": result.output,
                    "error": result.error,
                })
                .to_string()
            };
            self.emit(AgentEvent::ToolCompleted {
                step_id,
                result: result.clone(),
                ts: Utc::now(),
            })?;
            self.emit(AgentEvent::ObservationCreated {
                step_id,
                observation_id: ObservationId::new(),
                ts: Utc::now(),
            })?;
            let tool_message = ChatMessage {
                role: Role::Tool,
                content: observation.clone(),
                tool_call_id: Some(invocation.id),
                tool_name: Some(invocation.name.clone()),
                tool_calls: vec![],
            };
            self.session
                .push_tool_observation(tool_message, &assistant_message);
        }

        self.emit(AgentEvent::StepCompleted {
            step_id,
            outcome: StepOutcome {
                stop_reason: StopReason::ToolUse,
                text,
                tool_calls: call_ids,
                escalated: false,
            },
            ts: Utc::now(),
        })?;
        Ok(())
    }

    fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError> {
        self.state = self
            .state
            .transition(&event)
            .map_err(|error| AgentError::Transition(error.to_string()))?;
        self.bus.emit(event);
        Ok(())
    }

    fn record_error(&mut self, message: String) -> Result<(), AgentError> {
        self.emit(AgentEvent::Error {
            session_id: self.session.id,
            message,
            ts: Utc::now(),
        })
    }

    fn cancel_turn(&mut self, turn_id: TurnId) -> Result<(), AgentError> {
        self.emit(AgentEvent::TurnCancelled {
            session_id: self.session.id,
            turn_id,
            ts: Utc::now(),
        })
    }

    fn active_skills(&self, prompt: &str) -> Result<Vec<Skill>, String> {
        let mut active = BTreeMap::new();
        for name in self
            .agent_profile
            .default_skills
            .iter()
            .chain(self.workspace_profile.enabled_skills.iter())
        {
            let skill = self
                .skills
                .get(name)
                .ok_or_else(|| format!("configured skill `{name}` is not registered"))?;
            active.insert(skill.name.clone(), skill.clone());
        }
        for skill in self.skills.match_prompt(prompt) {
            active.insert(skill.name.clone(), skill.clone());
        }
        Ok(active.into_values().collect())
    }
}

pub struct KernelBuilder {
    providers: ModelRegistry,
    routing_policy: RoutingPolicy,
    tools: ToolRegistry,
    approval: Arc<dyn ApprovalPolicy>,
    broker: Arc<dyn SecretBroker>,
    gate: Arc<dyn QualityGate>,
    limits: KernelLimits,
    workspace_root: PathBuf,
    model_options: KernelModelOptions,
    agent_profile: AgentProfile,
    workspace_profile: Option<WorkspaceProfile>,
    skills: SkillRegistry,
    prompt_compiler: PromptCompiler,
    context_items: Vec<ContextItem>,
}

impl KernelBuilder {
    pub fn providers(mut self, providers: ModelRegistry) -> Self {
        self.providers = providers;
        self
    }

    pub fn routing_policy(mut self, routing_policy: RoutingPolicy) -> Self {
        self.routing_policy = routing_policy;
        self
    }

    pub fn tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }

    pub fn approval(mut self, approval: impl ApprovalPolicy + 'static) -> Self {
        self.approval = Arc::new(approval);
        self
    }

    pub fn broker(mut self, broker: impl SecretBroker + 'static) -> Self {
        self.broker = Arc::new(broker);
        self
    }

    pub fn gate(mut self, gate: impl QualityGate + 'static) -> Self {
        self.gate = Arc::new(gate);
        self
    }

    pub fn limits(mut self, limits: KernelLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn workspace_root(mut self, workspace_root: impl Into<PathBuf>) -> Self {
        self.workspace_root = workspace_root.into();
        self
    }

    pub fn model_options(mut self, model_options: KernelModelOptions) -> Self {
        self.model_options = model_options;
        self
    }

    pub fn agent_profile(mut self, profile: AgentProfile) -> Self {
        self.agent_profile = profile;
        self
    }

    pub fn workspace_profile(mut self, workspace: WorkspaceProfile) -> Self {
        self.workspace_profile = Some(workspace);
        self
    }

    pub fn skills(mut self, skills: SkillRegistry) -> Self {
        self.skills = skills;
        self
    }

    pub fn prompt_compiler(mut self, compiler: PromptCompiler) -> Self {
        self.prompt_compiler = compiler;
        self
    }

    pub fn context_items(mut self, items: impl IntoIterator<Item = ContextItem>) -> Self {
        self.context_items = items.into_iter().collect();
        self
    }

    pub fn build(self) -> Kernel {
        let cancellation = CancellationToken::new();
        let tool_context = ToolContext::new(
            self.workspace_root.clone(),
            self.broker.clone(),
            cancellation.clone(),
            self.limits.tool_timeout,
        );
        let workspace_profile = self.workspace_profile.unwrap_or_else(|| WorkspaceProfile {
            root: self.workspace_root.clone(),
            project_type: ProjectType::Unknown,
            languages: Vec::new(),
            frameworks: Vec::new(),
            build_commands: Vec::new(),
            test_commands: Vec::new(),
            conventions: Vec::new(),
            enabled_skills: Vec::new(),
            repository_instructions: Vec::new(),
        });
        let mut session = Session::new();
        session.working_memory = self.context_items;
        Kernel {
            session,
            state: KernelState::Idle,
            bus: EventBus::default(),
            scheduler: ModelScheduler::new(self.providers, self.routing_policy),
            tools: self.tools,
            approval: self.approval,
            broker: self.broker,
            gate: self.gate,
            limits: self.limits,
            cancellation,
            tool_context,
            model_options: self.model_options,
            agent_profile: self.agent_profile,
            workspace_profile,
            skills: self.skills,
            prompt_compiler: self.prompt_compiler,
            last_requests: Vec::new(),
        }
    }
}

impl Default for KernelBuilder {
    fn default() -> Self {
        Self {
            providers: ModelRegistry::new(),
            routing_policy: RoutingPolicy::default(),
            tools: ToolRegistry::new(),
            approval: Arc::new(AllowAllApproval),
            broker: Arc::new(NoopBroker),
            gate: Arc::new(HeuristicGate),
            limits: KernelLimits::default(),
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            model_options: KernelModelOptions::default(),
            agent_profile: AgentProfile::builtin_default(),
            workspace_profile: None,
            skills: SkillRegistry::default(),
            prompt_compiler: PromptCompiler::default(),
            context_items: Vec::new(),
        }
    }
}

fn serialized_system_tokens(estimator: &TokenEstimator, system: &str) -> Result<u64, AgentError> {
    serde_json::to_string(&Some(system))
        .map(|serialized| estimator.estimate(&serialized))
        .map_err(|error| AgentError::Context(format!("could not serialize system prompt: {error}")))
}

fn serialized_context_tokens(
    estimator: &TokenEstimator,
    messages: &[ChatMessage],
    tools: &[agent_core::ToolDefinition],
) -> Result<u64, AgentError> {
    serde_json::to_string(&(messages, tools))
        .map(|serialized| estimator.estimate(&serialized))
        .map_err(|error| {
            AgentError::Context(format!("could not serialize request context: {error}"))
        })
}

fn step_needs_tools(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    let word_sequence: Vec<_> = lower
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '\''
        })
        .filter(|word| !word.is_empty())
        .collect();
    let words: HashSet<_> = word_sequence.iter().copied().collect();
    let file_action = has_unnegated_english_action(
        &word_sequence,
        &[
            "list", "listing", "read", "reading", "write", "writing", "create", "delete", "search",
            "edit", "open", "show",
        ],
    ) && ["file", "files", "directory", "folder", "filesystem"]
        .iter()
        .any(|word| words.contains(word));
    let path_action = has_unnegated_english_action(
        &word_sequence,
        &[
            "read", "write", "create", "delete", "search", "edit", "open",
        ],
    ) && (lower.contains('/')
        || lower.contains(".rs")
        || lower.contains(".toml")
        || lower.contains(".json")
        || lower.contains(".yaml")
        || lower.contains(".yml"));
    let command_action = has_unnegated_english_action(&word_sequence, &["run", "execute"])
        && ["shell", "command", "script", "tests"]
            .iter()
            .any(|word| words.contains(word));
    let project_action = has_unnegated_english_action(&word_sequence, &["build", "test"])
        && ["project", "code", "workspace", "repository"]
            .iter()
            .any(|word| words.contains(word));
    let repository_action = has_unnegated_english_action(&word_sequence, &["inspect", "search"])
        && ["repository", "repo", "workspace"]
            .iter()
            .any(|word| words.contains(word));
    let explanatory = [
        "explain", "describe", "what", "meaning", "concept", "how", "why",
    ]
    .iter()
    .any(|word| words.contains(word));
    let coding_action = has_unnegated_english_action(
        &word_sequence,
        &["fix", "repair", "implement", "add", "update", "refactor"],
    ) && ["bug", "error", "test", "tests", "feature", "code", "module"]
        .iter()
        .any(|word| words.contains(word))
        && !explanatory;
    let chinese_file_action = has_unnegated_chinese_action(
        &lower,
        &[
            "列出", "查看", "读取", "读", "写入", "写", "创建", "删除", "搜索", "编辑",
        ],
    ) && lower.contains("文件");
    let chinese_command_action = has_unnegated_chinese_action(&lower, &["执行", "运行"])
        && (lower.contains("命令") || lower.contains("shell"));
    let chinese_project_action = has_unnegated_chinese_action(&lower, &["构建", "测试"])
        && (lower.contains("项目") || lower.contains("代码"));
    let chinese_repository_action = has_unnegated_chinese_action(&lower, &["检查", "搜索"])
        && (lower.contains("仓库") || lower.contains("代码库"));
    let chinese_explanatory = ["解释", "什么", "如何", "怎么", "为什么"]
        .iter()
        .any(|marker| lower.contains(marker));
    let chinese_coding_action = (has_unnegated_chinese_action(&lower, &["修复", "实现"])
        || (has_unnegated_chinese_action(&lower, &["添加", "更新", "重构"])
            && ["错误", "缺陷", "测试", "功能", "代码", "模块"]
                .iter()
                .any(|object| lower.contains(object))))
        && !chinese_explanatory;
    let actionable = file_action
        || path_action
        || command_action
        || project_action
        || repository_action
        || coding_action
        || chinese_file_action
        || chinese_command_action
        || chinese_project_action
        || chinese_repository_action
        || chinese_coding_action;
    actionable && !explanatory && !chinese_explanatory
}

fn has_unnegated_english_action(words: &[&str], actions: &[&str]) -> bool {
    words.iter().enumerate().any(|(index, word)| {
        let preceding = index
            .checked_sub(1)
            .and_then(|preceding| words.get(preceding))
            .copied();
        actions.contains(word)
            && !matches!(
                preceding,
                Some("don't" | "not" | "never" | "avoid" | "without")
            )
    })
}

fn has_unnegated_chinese_action(text: &str, actions: &[&str]) -> bool {
    actions.iter().any(|action| {
        text.match_indices(action).any(|(index, _)| {
            let prefix = text[..index].trim_end();
            !["不要", "别", "无需", "不必"]
                .iter()
                .any(|negation| prefix.ends_with(negation))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::step_needs_tools;

    #[test]
    fn tool_intent_requires_an_action_and_concrete_object() {
        for prompt in [
            "list files",
            "read the config file",
            "write a file",
            "create src/main.rs",
            "delete the old file",
            "search files for TODO",
            "edit Cargo.toml",
            "run a shell command",
            "execute command cargo fmt",
            "build the project",
            "test this code",
            "inspect the repository",
            "fix this bug",
            "implement feature",
            "repair failing test",
            "add a feature",
            "update code",
            "refactor module",
            "fix this bug without changing the API",
            "read the file; do not edit it",
            "列出文件",
            "读取配置文件",
            "写入文件",
            "创建文件",
            "删除文件",
            "搜索文件",
            "编辑文件",
            "执行命令",
            "运行 shell 命令",
            "构建项目",
            "测试代码",
            "检查仓库",
            "修复这个错误",
            "实现这个功能",
            "实现 M4",
            "修复登录问题",
            "修复登录问题，不要修改数据库",
            "实现功能，无需重构模块",
        ] {
            assert!(step_needs_tools(prompt), "expected tool intent: {prompt}");
        }

        for prompt in [
            "explain unit testing",
            "what is a build",
            "describe the command pattern",
            "explain the directory concept",
            "how do filesystems work",
            "解释单元测试",
            "什么是构建",
            "解释命令模式",
            "目录是什么概念",
            "explain how bug fixes work",
            "what does feature implementation mean",
            "describe a failing test",
            "解释如何修复错误",
            "什么是功能实现",
            "how should I fix this bug?",
            "why should I fix this bug?",
            "don't fix this bug",
            "do not implement this feature",
            "如何修复登录问题",
            "不要实现这个功能",
        ] {
            assert!(
                !step_needs_tools(prompt),
                "unexpected tool intent: {prompt}"
            );
        }
    }
}

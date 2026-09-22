use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_context::{CompileRequest, PromptCompiler};
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
use agent_skill::SkillRegistry;
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
        self.session.messages.push(ChatMessage {
            role: Role::User,
            content: user_text.to_owned(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: vec![],
        });
        self.session
            .remember(ContextItemKind::Message, user_text, "user");

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
            let token_budget = u64::from(
                selected
                    .capabilities
                    .context_window
                    .saturating_sub(max_tokens),
            );
            let matched_skills = self
                .skills
                .match_prompt(user_text)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            let compiled = match self.prompt_compiler.compile(CompileRequest {
                profile: &self.agent_profile,
                model: &selected.capabilities,
                workspace: &self.workspace_profile,
                skills: &matched_skills,
                tool_guidance: None,
                context_items: &self.session.working_memory,
                token_budget,
            }) {
                Ok(compiled) => compiled,
                Err(error) => {
                    let error = AgentError::Context(error.to_string());
                    self.record_error(error.to_string())?;
                    return Err(error);
                }
            };
            let system_tokens = u32::try_from(compiled.system_tokens).map_err(|_| {
                AgentError::Context("compiled system token count exceeds u32".into())
            })?;
            let context_tokens = u32::try_from(compiled.context_tokens).map_err(|_| {
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
                messages: request_messages(&self.session.messages, &compiled.context_items),
                tools: if step_context.needs_tools && selected.capabilities.supports_tools {
                    tool_definitions
                } else {
                    Vec::new()
                },
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
                    self.session.messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: text.clone(),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: vec![],
                    });
                    self.session
                        .remember(ContextItemKind::Message, text.clone(), "assistant");
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
        self.session.messages.push(ChatMessage {
            role: Role::Assistant,
            content: text.clone(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: tool_calls.clone(),
        });

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
            self.session.messages.push(ChatMessage {
                role: Role::Tool,
                content: observation.clone(),
                tool_call_id: Some(invocation.id),
                tool_name: Some(invocation.name.clone()),
                tool_calls: vec![],
            });
            self.session.remember(
                ContextItemKind::ToolObservation,
                observation,
                format!("tool://{}#{}", invocation.name, invocation.id),
            );
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

fn request_messages(messages: &[ChatMessage], context_items: &[ContextItem]) -> Vec<ChatMessage> {
    let selected: HashSet<_> = context_items
        .iter()
        .map(|item| item.content.as_str())
        .collect();
    let selected_tool_calls: HashSet<_> = messages
        .iter()
        .filter(|message| message.role == Role::Tool && selected.contains(message.content.as_str()))
        .filter_map(|message| message.tool_call_id)
        .collect();
    messages
        .iter()
        .filter(|message| {
            selected.contains(message.content.as_str())
                || message
                    .tool_calls
                    .iter()
                    .any(|call| selected_tool_calls.contains(&call.id))
        })
        .cloned()
        .collect()
}

fn step_needs_tools(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    let words: HashSet<_> = lower
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|word| !word.is_empty())
        .collect();
    let file_action = [
        "list", "listing", "read", "reading", "write", "writing", "open", "show",
    ]
    .iter()
    .any(|word| words.contains(word))
        && ["file", "files", "directory", "folder", "filesystem"]
            .iter()
            .any(|word| words.contains(word));
    let command_action = ["shell", "command", "run", "execute", "build", "test"]
        .iter()
        .any(|word| words.contains(word));
    const CHINESE: [&str; 12] = [
        "列出文件",
        "查看文件",
        "读取文件",
        "读文件",
        "写入文件",
        "写文件",
        "目录",
        "文件夹",
        "命令",
        "执行",
        "构建",
        "测试",
    ];
    file_action || command_action || CHINESE.iter().any(|needle| lower.contains(needle))
}

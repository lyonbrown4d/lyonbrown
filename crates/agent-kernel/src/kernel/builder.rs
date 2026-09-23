use super::*;

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
  storage: Option<SessionStore>,
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

  pub fn broker_arc(mut self, broker: Arc<dyn SecretBroker>) -> Self {
    self.broker = broker;
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

  pub fn storage(mut self, storage: SessionStore) -> Self {
    self.storage = Some(storage);
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
    let writer = self
      .storage
      .as_ref()
      .map(|storage| storage.writer(session.id));
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
      storage: self.storage,
      writer,
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
      storage: None,
    }
  }
}

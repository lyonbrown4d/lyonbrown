use super::*;

#[tokio::test]
async fn configured_and_triggered_skills_form_a_deterministic_union() {
  use agent_profile::AgentProfile;
  use agent_skill::{Skill, SkillRegistry};
  use agent_workspace::WorkspaceProfile;

  let provider = Arc::new(RoutingProvider::new());
  let registry = ModelRegistry::new();
  registry.register(provider.clone());
  let mut profile = AgentProfile::builtin_default();
  profile.default_skills = vec!["z-default".into(), "shared".into()];
  let mut workspace = WorkspaceProfile::detect(env!("CARGO_MANIFEST_DIR")).unwrap();
  workspace.enabled_skills = vec!["a-workspace".into(), "shared".into()];
  let skills = SkillRegistry::new([
    Skill::from_yaml("name: z-default\ninstructions: DEFAULT_MARKER\n").unwrap(),
    Skill::from_yaml("name: a-workspace\ninstructions: WORKSPACE_MARKER\n").unwrap(),
    Skill::from_yaml("name: shared\ntriggers: [hello]\ninstructions: SHARED_MARKER\n").unwrap(),
    Skill::from_yaml("name: prompt\ntriggers: [hello]\ninstructions: PROMPT_MARKER\n").unwrap(),
  ])
  .unwrap();
  let mut kernel = Kernel::builder()
    .providers(registry)
    .agent_profile(profile)
    .workspace_profile(workspace)
    .skills(skills)
    .build();

  kernel.run_turn("hello").await.unwrap();

  let system = provider.requests()[0].system.clone().unwrap();
  for marker in [
    "DEFAULT_MARKER",
    "WORKSPACE_MARKER",
    "SHARED_MARKER",
    "PROMPT_MARKER",
  ] {
    assert_eq!(system.matches(marker).count(), 1);
  }
  let matched_section = system.split("## Matched Skills").nth(1).unwrap();
  let headings = matched_section
    .lines()
    .filter(|line| line.starts_with("### "))
    .collect::<Vec<_>>();
  assert!(headings.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[tokio::test]
async fn unknown_configured_skill_is_a_logged_context_error() {
  use agent_profile::AgentProfile;
  let provider = Arc::new(RoutingProvider::new());
  let registry = ModelRegistry::new();
  registry.register(provider);
  let mut profile = AgentProfile::builtin_default();
  profile.default_skills = vec!["missing".into()];
  let mut kernel = Kernel::builder()
    .providers(registry)
    .agent_profile(profile)
    .build();

  let error = kernel.run_turn("hello").await.unwrap_err();

  assert!(matches!(error, agent_core::AgentError::Context(_)));
  assert!(error.to_string().contains("missing"));
  assert_eq!(kernel.event_log().last().unwrap().kind(), "Error");
}

struct HugeSchemaTool {
  definition: ToolDefinition,
}

impl HugeSchemaTool {
  fn new() -> Self {
    Self {
      definition: ToolDefinition {
        name: "huge".into(),
        description: "large schema".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "description": "x".repeat(16_384),
        }),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Read,
        requires_secrets: vec![],
      },
    }
  }
}

#[async_trait]
impl Tool for HugeSchemaTool {
  fn definition(&self) -> &ToolDefinition {
    &self.definition
  }

  async fn invoke(
    &self,
    _context: &ToolContext,
    invocation: &ToolInvocation,
  ) -> Result<ToolResult, ToolError> {
    Ok(ToolResult {
      invocation_id: invocation.id,
      ok: true,
      output: String::new(),
      truncated: false,
      duration_ms: 0,
      error: None,
    })
  }
}

#[tokio::test]
async fn oversized_tool_schema_cannot_bypass_the_input_budget() {
  let provider = Arc::new(RoutingProvider::with_context_window(2_048));
  let registry = ModelRegistry::new();
  registry.register(provider.clone());
  let tools = ToolRegistry::new();
  tools.register(Arc::new(HugeSchemaTool::new()));
  let mut kernel = Kernel::builder().providers(registry).tools(tools).build();

  let error = kernel.run_turn("list files").await.unwrap_err();

  assert!(matches!(error, agent_core::AgentError::Context(_)));
  assert!(provider.requests().is_empty());
  assert_eq!(kernel.event_log().last().unwrap().kind(), "Error");
}

#[tokio::test]
async fn secret_working_memory_never_reaches_provider_request() {
  let provider = Arc::new(RoutingProvider::new());
  let registry = ModelRegistry::new();
  registry.register(provider.clone());
  let mut secret = ContextItem::new(ContextItemKind::Memory, "do-not-send", 0);
  secret.classification = DataClassification::Secret;
  let document = ContextItem::new(ContextItemKind::Document, "public document", 1);
  let memory = ContextItem::new(ContextItemKind::Memory, "public memory", 1);
  let external_message = ContextItem::new(ContextItemKind::Message, "external message", 1);
  let external_observation =
    ContextItem::new(ContextItemKind::ToolObservation, "external observation", 1);
  let mut kernel = Kernel::builder()
    .providers(registry)
    .context_items([
      secret,
      document,
      memory,
      external_message,
      external_observation,
    ])
    .build();

  let events = kernel.run_turn("say hi").await.unwrap();

  let request = &provider.requests()[0];
  assert!(
    !request
      .system
      .as_deref()
      .unwrap_or_default()
      .contains("do-not-send")
  );
  assert!(
    request
      .messages
      .iter()
      .all(|message| !message.content.contains("do-not-send"))
  );
  assert!(request.messages.iter().any(|message| {
    message.role == agent_model::Role::System
      && message.content.contains("Document")
      && message.content.contains("public document")
  }));
  assert!(request.messages.iter().any(|message| {
    message.role == agent_model::Role::System
      && message.content.contains("Message")
      && message.content.contains("external message")
  }));
  assert!(request.messages.iter().any(|message| {
    message.role == agent_model::Role::System
      && message.content.contains("ToolObservation")
      && message.content.contains("external observation")
  }));
  assert!(request.messages.iter().any(|message| {
    message.role == agent_model::Role::System
      && message.content.contains("Memory")
      && message.content.contains("public memory")
  }));
  let prepared = events
    .iter()
    .find_map(|event| match event {
      AgentEvent::ContextPrepared {
        system_tokens,
        context_tokens,
        ..
      } => Some((u64::from(*system_tokens), u64::from(*context_tokens))),
      _ => None,
    })
    .unwrap();
  let estimator = agent_context::TokenEstimator::default();
  assert_eq!(
    prepared.0,
    estimator.estimate(&serde_json::to_string(&request.system).unwrap())
  );
  assert_eq!(
    prepared.1,
    estimator.estimate(
      &serde_json::to_string(&(request.messages.as_slice(), request.tools.as_slice())).unwrap()
    )
  );
  assert!(prepared.0 + prepared.1 <= u64::from(8_192 - request.max_tokens));
}

#[tokio::test]
async fn compilation_budget_failure_is_an_agent_error_and_is_logged() {
  let provider = Arc::new(RoutingProvider::with_context_window(64));
  let registry = ModelRegistry::new();
  registry.register(provider);
  let mut kernel = Kernel::builder().providers(registry).build();

  let error = kernel.run_turn("say hi").await.unwrap_err();

  assert!(matches!(error, agent_core::AgentError::Context(_)));
  assert_eq!(kernel.event_log().last().unwrap().kind(), "Error");
}

#[tokio::test]
async fn configured_model_and_generation_options_reach_the_provider() {
  let provider = Arc::new(RoutingProvider::new());
  let registry = ModelRegistry::new();
  registry.register(provider.clone());
  let mut kernel = Kernel::builder()
    .providers(registry)
    .model_options(KernelModelOptions {
      provider: Some("routing-test".into()),
      model: Some("worker".into()),
      max_tokens: Some(321),
      temperature: Some(0.4),
    })
    .build();

  kernel.run_turn("say hi").await.unwrap();

  let request = &provider.requests()[0];
  assert_eq!(request.model, "worker");
  assert_eq!(request.max_tokens, 321);
  assert_eq!(request.temperature, Some(0.4));
}

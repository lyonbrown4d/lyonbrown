use agent_decision::*;
use agent_runtime::RuntimeConfig;
use agent_test_support::{MockModelServer, StandardProtocol};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc, time::Duration};

pub fn policy() -> DecisionPolicy {
  DecisionPolicy {
    authorized_tools: [
      Tool::ReadFile,
      Tool::ListFiles,
      Tool::SearchFiles,
      Tool::WriteFile,
      Tool::BrowserObserve,
      Tool::BrowserAct,
      Tool::ComputerObserve,
      Tool::ComputerAct,
    ]
    .into(),
    available_capabilities: [
      Capability::FileRead,
      Capability::FileWrite,
      Capability::Browser,
      Capability::Computer,
    ]
    .into(),
    allowed_models: [
      ModelPolicy::General,
      ModelPolicy::Reasoning,
      ModelPolicy::Vision,
    ]
    .into(),
    distributed_allowed: true,
    allowed_targets: ["paired-worker".into()].into(),
    minimum_confidence: 0.75,
    classifier_timeout: Duration::from_secs(3),
  }
}
pub fn candidate(kind: &str, tools: &[&str]) -> Value {
  json!({"task_kind":kind,"model_policy":"general","allowed_tools":tools,
    "execution_mode":"local","target_hint":null,"risk_level":"low","confidence":0.9})
}
pub fn engine(
  server: &MockModelServer,
  protocol: StandardProtocol,
  policy: DecisionPolicy,
) -> Arc<DecisionEngine> {
  let config = RuntimeConfig {
    session_id: "decision-fixture".into(),
    owner_epoch: 1,
    provider: protocol.provider().into(),
    model: "classifier-fixture".into(),
    api_key: "synthetic-classifier-secret".into(),
    base_url: Some(server.base_url().into()),
    system_prompt: None,
    context_window: 16_384,
    max_output_tokens: 1024,
    supports_images: false,
  };
  Arc::new(DecisionEngine::new(policy, Some(Arc::new(RemoteClassifier::new(config)))).unwrap())
}
pub fn assert_fallback(plan: &ExecutionPlan, reason: FallbackReason) {
  assert_eq!(plan.source, DecisionSource::Fallback { reason });
  assert!(plan.allowed_tools.is_empty());
  assert!(plan.required_capabilities.is_empty());
  assert_eq!(plan.execution_mode, ExecutionMode::Local);
  assert!(plan.target_hint.is_none());
  assert_eq!(plan.model_policy, ModelPolicy::General);
  assert_eq!(plan.risk_level, RiskLevel::High);
  assert!(plan.confidence.is_none());
}
pub fn tools(items: &[Tool]) -> BTreeSet<Tool> {
  items.iter().copied().collect()
}

use std::sync::Arc;

use agent_model::{Complexity, ModelTaskRouter, StepContext, TaskRouter};
use provider_mock::{MockProvider, ScriptedStep};

#[tokio::test]
async fn model_router_maps_typed_decision_to_complexity() {
  let provider = Arc::new(MockProvider::scripted(vec![ScriptedStep::text("HIGH")]));
  let router = ModelTaskRouter::new(provider.clone(), "mock-model");

  let complexity = router
    .classify(&StepContext::new("重构这个 Rust workspace"))
    .await
    .unwrap();

  assert_eq!(complexity, Complexity::High);
  let requests = provider.requests();
  assert_eq!(requests.len(), 1);
  assert_eq!(requests[0].temperature, Some(0.0));
  assert!(requests[0].system.as_deref().unwrap().contains("LOW"));
}

#[tokio::test]
async fn model_router_rejects_untyped_output() {
  let provider = Arc::new(MockProvider::scripted(vec![ScriptedStep::text(
    "maybe difficult",
  )]));
  let router = ModelTaskRouter::new(provider, "mock-model");

  let error = router
    .classify(&StepContext::new("ambiguous"))
    .await
    .unwrap_err();

  assert!(error.to_string().contains("LOW, MEDIUM, or HIGH"));
}

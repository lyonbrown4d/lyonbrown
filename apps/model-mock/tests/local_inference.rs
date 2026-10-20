use agent_inference::{InferenceBackend, ModelInfo, ModelRegistry};
use agent_test_support::{MockGeneration, MockInferenceBackend, MockInferenceStep};
use decision_core::*;
use decision_model::ModelDecisionBackend;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn info() -> ModelInfo {
  ModelInfo {
    id: "synthetic-local".into(),
    backend: "ignored".into(),
    context_window: 4096,
    supports_tools: false,
    supports_images: false,
  }
}
#[tokio::test]
async fn local_registry_and_typed_decision_use_real_inference_boundary_without_weights() {
  let output=json!({"output":{"operation":"predicate","value":{"value":true,"confidence":0.95}},"confidence":0.95,"reason_codes":["SYNTHETIC"]}).to_string();
  let backend = Arc::new(
    MockInferenceBackend::new(
      info(),
      vec![MockGeneration::text(output), MockGeneration::stalled()],
    )
    .unwrap(),
  );
  let registry = Arc::new(ModelRegistry::new(1).unwrap());
  registry.register(backend.clone(), 1).await.unwrap();
  let managed = registry.backend("synthetic-local").await.unwrap();
  assert_eq!(managed.info().backend, "synthetic/model-mock");
  let policy = Policy::default();
  let decision_budget_ms = policy.max_deadline_ms;
  let engine = DecisionEngine::new(
    policy,
    vec![Arc::new(ModelDecisionBackend::local(managed.clone()))],
  )
  .unwrap();
  let mut request = DecisionRequest::new(
    "local",
    Operation::Predicate(PredicateInput {
      context: "synthetic".into(),
      question: "valid?".into(),
    }),
  );
  // Verify the registry/adapter contract within the existing policy budget,
  // rather than making the request's 100 ms default a host-latency assertion.
  request.deadline_ms = decision_budget_ms;
  let decision = engine.decide(&request, &DecisionCancellation::new()).await;
  assert_eq!(
    decision.status,
    DecisionStatus::Accepted,
    "decision: {decision:?}; inference: {:?}",
    backend.counts()
  );
  assert!(
    tokio::time::timeout(
      Duration::from_millis(20),
      model_provider::classify_local_json(
        &managed,
        "system".into(),
        "user".into(),
        CancellationToken::new()
      )
    )
    .await
    .is_err()
  );
  assert_eq!(backend.counts().requests, 2);
  assert_eq!(backend.counts().completed, 1);
  assert_eq!(backend.counts().cancelled, 1);
  // A cancelled attempt releases registry permits; exhausted scripts fail explicitly.
  assert!(
    tokio::time::timeout(
      Duration::from_secs(1),
      model_provider::classify_local_json(
        &managed,
        "system".into(),
        "user".into(),
        CancellationToken::new()
      )
    )
    .await
    .unwrap()
    .is_err()
  );
  assert_eq!(backend.counts().failed, 1);
}
#[tokio::test]
async fn local_chunk_errors_and_nonstop_finish_are_rejected_by_classifier() {
  let mut error = MockGeneration::text("{}");
  error.steps = vec![MockInferenceStep::Error("synthetic failure".into())];
  let mut length = MockGeneration::text("{}");
  length.summary.reason = agent_inference::FinishReason::Length;
  let backend: Arc<dyn InferenceBackend> =
    Arc::new(MockInferenceBackend::new(info(), vec![error, length]).unwrap());
  for _ in 0..2 {
    assert!(
      model_provider::classify_local_json(
        &backend,
        "system".into(),
        "user".into(),
        CancellationToken::new()
      )
      .await
      .is_err()
    );
  }
}

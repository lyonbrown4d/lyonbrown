use decision_core::{
  BackendError, DecisionCancellation, DecisionEngine, DecisionOutput, DecisionRequest,
  DecisionStatus, ModelCandidate, ModelTier, Operation, Policy, RouteInput,
};
use decision_semantic::{
  EmbeddingDescriptor, EmbeddingProvider, SemanticBackend, SemanticConfig, SemanticExample,
};
use std::sync::Arc;

struct Axis;
impl EmbeddingProvider for Axis {
  fn descriptor(&self) -> EmbeddingDescriptor {
    EmbeddingDescriptor {
      model_revision: "synthetic-v1".into(),
      preprocessing_revision: "exact-text-v1".into(),
      dimensions: 2,
      local: true,
    }
  }
  fn embed(&self, text: &str) -> Result<Vec<f32>, BackendError> {
    Ok(if text.contains("bonjour") {
      vec![0.0, 1.0]
    } else {
      vec![1.0, 0.0]
    })
  }
}
#[tokio::test]
async fn embedded_engine_consumes_semantics_and_enforces_authorized_model_filter() {
  let backend = SemanticBackend::load(
    SemanticConfig {
      index_revision: "axis-v1".into(),
      config_revision: "test-v1".into(),
      minimum_score: 0.7,
      minimum_margin: 0.1,
      negative_penalty: 0.0,
      max_concurrent_inference: 1,
      max_input_bytes: 4096,
      examples: vec![
        SemanticExample {
          label: "coding".into(),
          text: "inspect lock contention".into(),
        },
        SemanticExample {
          label: "chat".into(),
          text: "bonjour".into(),
        },
      ],
      hard_negatives: vec![],
    },
    Arc::new(Axis),
  )
  .await
  .expect("corpus");
  let engine = DecisionEngine::new(Policy::default(), vec![Arc::new(backend)]).expect("engine");
  let make_model = |key: &str, local| ModelCandidate {
    key: key.into(),
    tier: ModelTier::Advanced,
    local,
    capabilities: ["tools".into()].into(),
    context_tokens: Some(4096),
    estimated_cost_microusd: Some(100),
  };
  let mut request = DecisionRequest::new(
    "route",
    Operation::RouteModel(RouteInput {
      text: "inspect lock contention".into(),
      candidates: vec![make_model("remote", false), make_model("local", true)],
      ..Default::default()
    }),
  );
  // This contract includes a blocking embedding worker; host scheduling is not under test.
  request.deadline_ms = Policy::default().max_deadline_ms;
  request.constraints.local_only = true;
  request
    .constraints
    .required_capabilities
    .insert("tools".into());
  let result = engine.decide(&request, &DecisionCancellation::new()).await;
  assert_eq!(result.status, DecisionStatus::Accepted, "{result:?}");
  let DecisionOutput::RouteModel(advice) = &result.output else {
    panic!("route advice expected: {result:?}")
  };
  assert_eq!(advice.task_domain, "coding");
  assert_eq!(advice.candidate_model_keys, vec!["local"]);
  assert!(advice.required_capabilities.contains("tools"));
  assert_eq!(result.provenance.index_revision.as_deref(), Some("axis-v1"));
  assert_eq!(result.provenance.method, "semantic-cosine");
}

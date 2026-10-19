use async_trait::async_trait;
use decision_core::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Stub {
  pub output: DecisionOutput,
  pub confidence: f64,
  pub local: bool,
  pub calls: Arc<AtomicUsize>,
  pub never_complete: bool,
  pub cancel: Option<DecisionCancellation>,
}
impl Stub {
  pub fn output(output: DecisionOutput) -> Self {
    Self {
      output,
      confidence: 0.9,
      local: true,
      calls: Arc::default(),
      never_complete: false,
      cancel: None,
    }
  }
}
#[async_trait]
impl DecisionBackend for Stub {
  fn descriptor(&self) -> BackendDescriptor {
    BackendDescriptor {
      name: "test".into(),
      version: "backend-v1".into(),
      index_revision: Some("index-v2".into()),
      local: self.local,
    }
  }
  fn supports(&self, _: &Operation) -> bool {
    true
  }
  async fn evaluate(
    &self,
    _: &DecisionRequest,
    _: &PolicySnapshot,
    _: &DecisionCancellation,
  ) -> Result<BackendDecision, BackendError> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    if let Some(cancel) = &self.cancel {
      cancel.cancel();
    }
    if self.never_complete {
      std::future::pending::<()>().await;
    }
    Ok(BackendDecision {
      output: self.output.clone(),
      confidence: self.confidence,
      reason_codes: vec!["TEST_EVIDENCE".into()],
    })
  }
}
#[allow(dead_code)]
pub fn model(key: &str, local: bool) -> ModelCandidate {
  ModelCandidate {
    key: key.into(),
    tier: ModelTier::Balanced,
    local,
    capabilities: ["tools".into()].into(),
    context_tokens: Some(8192),
    estimated_cost_microusd: Some(100),
  }
}
#[allow(dead_code)]
pub fn route(text: &str, candidates: Vec<ModelCandidate>) -> DecisionRequest {
  DecisionRequest::new(
    "test",
    Operation::RouteModel(RouteInput {
      text: text.into(),
      candidates,
      ..RouteInput::default()
    }),
  )
}
pub async fn decide(
  request: &DecisionRequest,
  backends: Vec<Arc<dyn DecisionBackend>>,
) -> DecisionResult {
  DecisionEngine::new(Policy::default(), backends)
    .expect("valid policy")
    .decide(request, &DecisionCancellation::new())
    .await
}

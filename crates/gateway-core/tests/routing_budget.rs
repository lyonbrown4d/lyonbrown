mod support;
use async_trait::async_trait;
use decision_core::{
  BackendDecision, BackendDescriptor, BackendError, Complexity, DecisionBackend,
  DecisionCancellation, DecisionEngine, DecisionOutput, DecisionRequest, ModelTier, Operation,
  Policy, PolicySnapshot, RouteAdvice,
};
use gateway_core::Gateway;
use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};
use support::*;

struct DelayedRoute(Arc<AtomicBool>);
#[async_trait]
impl DecisionBackend for DelayedRoute {
  fn descriptor(&self) -> BackendDescriptor {
    BackendDescriptor {
      name: "delayed-fixture".into(),
      version: "1".into(),
      index_revision: None,
      local: true,
    }
  }
  fn supports(&self, operation: &Operation) -> bool {
    matches!(operation, Operation::RouteModel(_))
  }
  async fn evaluate(
    &self,
    request: &DecisionRequest,
    _: &PolicySnapshot,
    _: &DecisionCancellation,
  ) -> Result<BackendDecision, BackendError> {
    tokio::time::sleep(Duration::from_millis(250)).await;
    self.0.store(true, Ordering::SeqCst);
    Ok(BackendDecision {
      output: DecisionOutput::RouteModel(RouteAdvice {
        task_domain: "coding".into(),
        complexity: Complexity::Medium,
        recommended_tier: ModelTier::Balanced,
        required_capabilities: request.constraints.required_capabilities.clone(),
        candidate_model_keys: vec!["public-model".into()],
      }),
      confidence: 0.95,
      reason_codes: vec!["SYNTHETIC_ROUTE".into()],
    })
  }
}

#[tokio::test]
async fn configured_first_byte_budget_includes_routing_and_expiry_never_dispatches_upstream() {
  for first_byte_ms in [5000, 30] {
    let (capture, upstream) = mock_json().await;
    let completed = Arc::new(AtomicBool::new(false));
    let engine = Arc::new(
      DecisionEngine::new(
        Policy::default(),
        vec![Arc::new(DelayedRoute(completed.clone()))],
      )
      .expect("engine"),
    );
    let audit = Arc::new(Audit::default());
    let mut settings = config(&upstream.base);
    settings.limits.first_byte_ms = first_byte_ms;
    settings.limits.total_ms = 10000;
    let gateway = Gateway::new(settings, engine, audit.clone()).expect("gateway");
    let server = serve(gateway.router()).await;
    let mut body = completion("router/auto");
    body["messages"][0]["content"] = serde_json::json!("opaque request");
    let response = post(&server.base, body).send().await.expect("response");
    if first_byte_ms > 100 {
      assert_eq!(response.status(), 200);
      assert!(
        completed.load(Ordering::SeqCst),
        "configured budget must permit the route backend to finish"
      );
      let value: serde_json::Value = response.json().await.expect("completion JSON");
      assert_eq!(value["model"], "actual-upstream-model");
      assert_eq!(capture.calls.load(Ordering::SeqCst), 1);
    } else {
      assert_eq!(response.status(), 504);
      let value: serde_json::Value = response.json().await.expect("error JSON");
      assert_eq!(value["error"]["code"], "routing_timeout");
      assert_eq!(capture.calls.load(Ordering::SeqCst), 0);
      assert!(audit.records.lock().expect("audit").is_empty());
    }
  }
}

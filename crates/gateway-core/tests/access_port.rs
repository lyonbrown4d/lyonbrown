mod support;
use async_trait::async_trait;
use decision_core::{DecisionEngine, Policy};
use gateway_core::{AccessControl, AccessError, Gateway};
use std::sync::Arc;
use support::*;

struct Denied;
#[async_trait]
impl AccessControl for Denied {
  async fn authorize(&self, _: &str) -> Result<bool, AccessError> {
    Ok(false)
  }
}
#[tokio::test]
async fn supplied_access_port_is_authoritative_over_configured_bearer() {
  let (capture, upstream) = mock_json().await;
  let engine = Arc::new(DecisionEngine::new(Policy::default(), vec![]).expect("engine"));
  let audit = Arc::new(Audit::default());
  let gateway = Gateway::new_with_access(
    config(&upstream.base),
    engine,
    audit.clone(),
    Arc::new(Denied),
  )
  .expect("gateway");
  let server = serve(gateway.router()).await;
  let response = post(&server.base, completion("public-model"))
    .send()
    .await
    .expect("response");
  assert_eq!(response.status(), 401);
  assert_eq!(capture.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
  assert!(audit.records.lock().expect("audit").is_empty());
}

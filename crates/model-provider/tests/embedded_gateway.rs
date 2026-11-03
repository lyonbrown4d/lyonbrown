use axum::{Json, Router, routing::post};
use futures::StreamExt;
use gateway_core::{
  AuditError, AuditRecord, AuditSink, AuditStatus, Gateway, GatewayConfig, Limits, ModelConfig,
  Secret,
};
use rig_core::completion::CompletionRequest;
use std::{
  collections::BTreeSet,
  sync::{Arc, Mutex},
};

#[derive(Default)]
struct Audit(Mutex<Vec<AuditRecord>>);
#[async_trait::async_trait]
impl AuditSink for Audit {
  async fn record(&self, record: AuditRecord) -> Result<(), AuditError> {
    self.0.lock().map_err(|_| AuditError)?.push(record);
    Ok(())
  }
}
#[tokio::test]
async fn rig_chat_uses_in_process_gateway_authentication_and_audit() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("mock listener");
  let address = listener.local_addr().expect("address");
  let server = tokio::spawn(async move {
    axum::serve(listener, Router::new().route("/v1/chat/completions", post(|Json(body): Json<serde_json::Value>| async move {
      assert_eq!(body["model"], "actual");
      ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], "data: {\"id\":\"reply\",\"object\":\"chat.completion.chunk\",\"model\":\"actual\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"reply\",\"object\":\"chat.completion.chunk\",\"model\":\"actual\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
    }))).await.expect("serve");
  });
  let audit = Arc::new(Audit::default());
  let token = "test-token-long-enough-for-gateway-auth";
  let config = GatewayConfig {
    token: Secret::new(token),
    models: vec![ModelConfig {
      id: "test".into(),
      upstream_model: "actual".into(),
      api_base: format!("http://{address}/v1"),
      api_key: None,
      api_key_env: None,
      tier: decision_core::ModelTier::Balanced,
      local_inference: true,
      capabilities: BTreeSet::from(["text".into()]),
      context_tokens: Some(65536),
    }],
    endpoints: vec![],
    groups: vec![],
    limits: Limits::default(),
    constraints: Default::default(),
  };
  let engine = Arc::new(
    decision_core::DecisionEngine::new(decision_core::Policy::default(), vec![]).expect("engine"),
  );
  let gateway = Gateway::new(config, engine, audit.clone()).expect("gateway");
  let model = model_provider::gateway_model(gateway, "test", token).expect("model");
  let mut stream = model
    .stream(CompletionRequest::new("hello").max_tokens(32))
    .expect("stream");
  while let Some(item) = stream.next().await {
    item.expect("stream item");
  }
  let _ = stream.finish().await.expect("completion");
  let records = audit.0.lock().expect("audit");
  assert_eq!(records.len(), 2);
  assert!(matches!(records[1].status, AuditStatus::Succeeded));
  server.abort();
}

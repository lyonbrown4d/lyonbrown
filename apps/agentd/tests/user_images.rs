#[allow(dead_code)]
mod support;
use agent_media::ImageAttachment;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use base64::Engine;
use serde_json::{Value, json};
use support::TestNode;
const PNG: &[u8] = include_bytes!("../../../crates/agent-media/tests/fixtures/one.png");
fn has_image(value: &Value, encoded: &str) -> bool {
  match value {
    Value::Object(fields) => fields.iter().any(|(key, value)| {
      ((key == "url" || key == "image_url" || key == "data")
        && value
          .as_str()
          .is_some_and(|text| text == encoded || text.ends_with(&format!("base64,{encoded}"))))
        || has_image(value, encoded)
    }),
    Value::Array(values) => values.iter().any(|value| has_image(value, encoded)),
    _ => false,
  }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicitly_submitted_images_reach_real_protocol_adapters_and_stay_out_of_events() {
  for protocol in StandardProtocol::ALL {
    let route=json!({"task_kind":"code","model_policy":"general","allowed_tools":["read_file"],"execution_mode":"local","target_hint":null,"risk_level":"low","confidence":0.99}).to_string();
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route)])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![text_reply(protocol, "synthetic image accepted")])
      .await
      .unwrap();
    let node = TestNode::start_configured(&main, &classifier, protocol, |config| {
      config.supports_images = true
    })
    .await;
    let image = ImageAttachment::from_bytes(PNG.to_vec()).unwrap();
    let request = agent_core::new_id();
    let run = node
      .client
      .start_run_with_attachments_owned(
        &node.session_id,
        1,
        "Describe the fixture",
        &request,
        std::slice::from_ref(&image),
      )
      .await
      .unwrap();
    let terminal = node.terminal(&run.id).await;
    assert_eq!(
      terminal.status, "completed",
      "{protocol:?}: {}",
      terminal.error
    );
    assert_eq!(terminal.result, "synthetic image accepted");
    assert_eq!(
      node.daemon.store.run_media(&run.id).await.unwrap(),
      vec![image.clone()]
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(PNG);
    let requests = main.requests().await;
    assert_eq!(requests.len(), 1);
    assert!(
      has_image(&requests[0].body, &encoded),
      "{protocol:?} image not sent through adapter"
    );
    let events = node.daemon.store.events(&run.id, 0, 1000).await.unwrap();
    assert!(
      events
        .iter()
        .all(|event| !event.payload_json.contains(&encoded))
    );
    let same = node
      .client
      .start_run_with_attachments_owned(
        &node.session_id,
        1,
        "Describe the fixture",
        &request,
        &[image],
      )
      .await
      .unwrap();
    assert_eq!(same.id, run.id);
    assert_eq!(
      main.requests().await.len(),
      1,
      "acknowledgement retry dispatched another model request"
    );
    node.close().await;
  }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_image_rpc_is_rejected_before_run_admission() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![]).await.unwrap();
  let main = MockModelServer::start(vec![]).await.unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  let mut image = ImageAttachment::from_bytes(PNG.to_vec()).unwrap();
  image.sha256 = "0".repeat(64);
  let mut raw = agent_protocol::v1::client_service_client::ClientServiceClient::connect(format!(
    "http://{}",
    node.daemon.config.listen
  ))
  .await
  .unwrap();
  let mut request = tonic::Request::new(agent_protocol::v1::StartRunWithAttachmentsRequest {
    session_id: node.session_id.clone(),
    owner_epoch: 1,
    prompt: "fixture".into(),
    idempotency_key: agent_core::new_id(),
    images: vec![agent_protocol::v1::RunImageAttachment {
      mime_type: image.mime_type,
      data: image.data,
      sha256: image.sha256,
    }],
  });
  request.metadata_mut().insert(
    "authorization",
    format!("Bearer {}", node.daemon.auth_token())
      .parse()
      .unwrap(),
  );
  let error = raw.start_run_with_attachments(request).await.unwrap_err();
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(
    node
      .daemon
      .store
      .runs(&node.session_id)
      .await
      .unwrap()
      .is_empty()
  );
  assert!(main.requests().await.is_empty());
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capability_rejection_precedes_admission_but_cannot_invalidate_an_existing_ack() {
  let protocol = StandardProtocol::ChatCompletions;
  let route=json!({"task_kind":"code","model_policy":"general","allowed_tools":["read_file"],"execution_mode":"local","target_hint":null,"risk_level":"low","confidence":0.99}).to_string();
  let classifier = MockModelServer::start(vec![text_reply(protocol, &route)])
    .await
    .unwrap();
  let main = MockModelServer::start(vec![text_reply(protocol, "accepted")])
    .await
    .unwrap();
  let node = TestNode::start_configured(&main, &classifier, protocol, |config| {
    config.supports_images = true
  })
  .await;
  let image = ImageAttachment::from_bytes(PNG.to_vec()).unwrap();
  let key = agent_core::new_id();
  let run = node
    .client
    .start_run_with_attachments_owned(
      &node.session_id,
      1,
      "fixture",
      &key,
      std::slice::from_ref(&image),
    )
    .await
    .unwrap();
  assert_eq!(node.terminal(&run.id).await.status, "completed");
  // Same account/store, changed daemon capability; only the public RPC is used.
  let mut disabled = node.daemon.clone();
  disabled.config.supports_images = false;
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let service = agentd::client_service(disabled);
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(service)
      .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
      .await
      .unwrap();
  });
  let client = agent_client::AgentClient::connect(&endpoint, node.daemon.auth_token())
    .await
    .unwrap();
  let same = client
    .start_run_with_attachments_owned(
      &node.session_id,
      1,
      "fixture",
      &key,
      std::slice::from_ref(&image),
    )
    .await
    .unwrap();
  assert_eq!(same.id, run.id);
  assert!(
    client
      .start_run_with_attachments_owned(
        &node.session_id,
        1,
        "fixture",
        &agent_core::new_id(),
        &[image]
      )
      .await
      .is_err()
  );
  assert_eq!(
    node
      .daemon
      .store
      .runs(&node.session_id)
      .await
      .unwrap()
      .len(),
    1
  );
  assert_eq!(main.requests().await.len(), 1);
  server.abort();
  node.close().await;
}

mod support;

use agent_mcp::{McpLimits, McpManager};
use agent_runtime::{RuntimeError, RuntimeTool};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};
use support::{Fixture, approval, call, server};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn real_protocol_calls_use_cached_contract_and_exact_authorization() {
  for json_response in [true, false] {
    let fixture = Fixture::default();
    let (config, stop) = server(fixture.clone(), json_response).await;
    let approval = approval(true);
    let manager = McpManager::connect(vec![config], approval.clone(), McpLimits::default())
      .await
      .expect("connect SDK server");
    let schema = manager.definition();
    assert_eq!(schema.name, "mcp_call");
    assert_eq!(
      schema.parameters["properties"]["call"]["oneOf"][0]["properties"]["arguments"]["properties"]
        ["message"]["$ref"],
      "#/properties/call/oneOf/0/properties/arguments/$defs/Message"
    );
    let definition_validator =
      jsonschema::validator_for(&schema.parameters).expect("relocated model-facing schema");
    assert!(definition_validator.is_valid(&call(json!("valid")).arguments));
    assert!(!definition_validator.is_valid(&call(json!(123)).arguments));
    let output = manager
      .execute(call(json!("hello")), CancellationToken::new())
      .await
      .expect("MCP call");
    assert!(!output.is_error);
    let value: Value = serde_json::from_str(&output.text).expect("untrusted envelope");
    assert_eq!(value["source"], "untrusted_mcp_server");
    assert_eq!(value["result"]["content"][0]["text"], "hello");
    assert_eq!(
      fixture.arguments.lock().await.as_slice(),
      &[json!({"message": "hello"})]
    );
    let seen = approval.seen.lock().await;
    assert_eq!(seen[0].call_id, "intent-123");
    assert_eq!(seen[0].server_id, "fixture");
    assert_eq!(seen[0].tool_name, "echo");
    assert_eq!(seen[0].arguments, json!({"message": "hello"}));
    manager.shutdown().await;
    stop.cancel();
  }
}

#[tokio::test]
async fn read_only_annotations_do_not_bypass_authorization() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), true).await;
  let approval = approval(false);
  let manager = McpManager::connect(vec![config], approval.clone(), McpLimits::default())
    .await
    .expect("connect");
  let denied = manager
    .execute(call(json!("hello")), CancellationToken::new())
    .await;
  assert!(matches!(denied, Err(RuntimeError::Tool(_))));
  assert_eq!(approval.seen.lock().await.len(), 1);
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
  manager.shutdown().await;
  stop.cancel();
}

#[tokio::test]
async fn schema_allowlist_and_precancellation_send_no_tool_request() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), true).await;
  let approval = approval(true);
  let manager = McpManager::connect(vec![config], approval.clone(), McpLimits::default())
    .await
    .expect("connect");
  assert!(
    manager
      .execute(call(json!(123)), CancellationToken::new())
      .await
      .is_err()
  );
  let mut request = call(json!("hello"));
  request.arguments["call"]["tool"] = json!("other");
  assert!(
    manager
      .execute(request, CancellationToken::new())
      .await
      .is_err()
  );
  let cancellation = CancellationToken::new();
  cancellation.cancel();
  assert!(
    manager
      .execute(call(json!("hello")), cancellation)
      .await
      .is_err()
  );
  assert!(approval.seen.lock().await.is_empty());
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
  manager.shutdown().await;
  stop.cancel();
}

#[tokio::test]
async fn server_errors_and_oversize_results_are_explicit() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), true).await;
  let manager = McpManager::connect(vec![config], approval(true), McpLimits::default())
    .await
    .expect("connect");
  let output = manager
    .execute(call(json!("error")), CancellationToken::new())
    .await
    .expect("server error output");
  assert!(output.is_error);
  assert!(output.text.contains("fixture rejected"));
  assert!(matches!(
    manager
      .execute(call(json!("oversize")), CancellationToken::new())
      .await,
    Err(RuntimeError::Tool(_))
  ));
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 2);
  manager.shutdown().await;
  stop.cancel();
}

#[tokio::test]
async fn wire_budget_rejects_before_json_decode_and_does_not_replay() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), true).await;
  let manager = McpManager::connect(vec![config], approval(true), McpLimits::default())
    .await
    .expect("connect");
  let result = manager
    .execute(call(json!("wire_oversize")), CancellationToken::new())
    .await;
  assert!(matches!(result, Err(RuntimeError::Uncertain(_))));
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
  manager.shutdown().await;
  stop.cancel();
}

#[tokio::test]
async fn run_specific_authorization_ports_share_connections_without_context_leakage() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), true).await;
  let manager = Arc::new(
    McpManager::connect(vec![config], approval(false), McpLimits::default())
      .await
      .expect("connect"),
  );
  let denied_port = approval(false);
  let allowed_port = approval(true);
  let denied = manager.tool_for(denied_port.clone());
  let allowed = manager.tool_for(allowed_port.clone());
  assert!(
    denied
      .execute(call(json!("denied")), CancellationToken::new())
      .await
      .is_err()
  );
  assert!(
    allowed
      .execute(call(json!("allowed")), CancellationToken::new())
      .await
      .is_ok()
  );
  assert_eq!(
    denied_port.seen.lock().await[0].arguments,
    json!({"message": "denied"})
  );
  assert_eq!(
    allowed_port.seen.lock().await[0].arguments,
    json!({"message": "allowed"})
  );
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
  drop(denied);
  drop(allowed);
  Arc::try_unwrap(manager)
    .ok()
    .expect("unique manager")
    .shutdown()
    .await;
  stop.cancel();
}

#[tokio::test]
async fn untrusted_server_cannot_sample_models_or_elicit_user_input() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), false).await;
  let manager = McpManager::connect(vec![config], approval(true), McpLimits::default())
    .await
    .expect("connect");
  let result = manager
    .execute(call(json!("callbacks")), CancellationToken::new())
    .await
    .expect("callback result");
  assert!(
    result
      .text
      .contains("sampling_denied=true;elicitation_denied=true")
  );
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
  manager.shutdown().await;
  stop.cancel();
}

#[tokio::test]
async fn image_output_limit_applies_to_actual_decoded_bytes() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), true).await;
  let limits = McpLimits {
    max_image_bytes: 8,
    ..Default::default()
  };
  let manager = McpManager::connect(vec![config], approval(true), limits)
    .await
    .expect("connect");
  assert!(matches!(
    manager
      .execute(call(json!("image")), CancellationToken::new())
      .await,
    Err(RuntimeError::Tool(_))
  ));
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
  manager.shutdown().await;
  stop.cancel();
}

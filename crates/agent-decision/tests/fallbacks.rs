mod support;
use agent_decision::*;
use agent_test_support::{
  MockModelServer, ScriptedReply, StandardProtocol, text_reply, tool_reply, truncated_reply,
};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn malformed_ambiguous_and_low_confidence_output_is_tool_free() {
  for protocol in StandardProtocol::ALL {
    let valid = candidate("code", &["write_file"]);
    let mut low = valid.clone();
    low["confidence"] = json!(0.7499);
    let mut high = valid.clone();
    high["confidence"] = json!(1.1);
    let mut unknown = valid.clone();
    unknown["approved"] = json!(true);
    let mut enum_value = valid.clone();
    enum_value["allowed_tools"] = json!(["root_shell"]);
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("risk_level");
    let mut null = valid.clone();
    null["confidence"] = json!(null);
    let cases = [
      ("not JSON".into(), FallbackReason::InvalidOutput),
      (
        format!("```json\n{valid}\n```"),
        FallbackReason::InvalidOutput,
      ),
      (format!("{valid}{valid}"), FallbackReason::InvalidOutput),
      (
        "{\"task_kind\":\"chat\",\"task_kind\":\"code\"}".into(),
        FallbackReason::InvalidOutput,
      ),
      (unknown.to_string(), FallbackReason::InvalidOutput),
      (enum_value.to_string(), FallbackReason::InvalidOutput),
      (missing.to_string(), FallbackReason::InvalidOutput),
      (null.to_string(), FallbackReason::InvalidOutput),
      (high.to_string(), FallbackReason::InvalidOutput),
      (low.to_string(), FallbackReason::LowConfidence),
    ];
    for (response, reason) in cases {
      let server = MockModelServer::start(vec![text_reply(protocol, &response)])
        .await
        .unwrap();
      let plan = engine(&server, protocol, policy())
        .decide("fixture request", None, CancellationToken::new())
        .await
        .unwrap();
      assert_fallback(&plan, reason);
      assert_eq!(server.requests().await.len(), 1);
    }
  }
}

#[tokio::test]
async fn transport_errors_truncation_and_tool_calls_never_trigger_retry_or_action() {
  for protocol in StandardProtocol::ALL {
    for reply in [
      ScriptedReply::status(401, json!({"error":{"message":"synthetic private detail"}})),
      ScriptedReply::status(429, json!({"error":{"message":"rate limited"}}))
        .header("retry-after", "0"),
      ScriptedReply::status(503, json!({"error":{"message":"unavailable"}})),
      ScriptedReply::raw_sse(b"data: {broken json}\n\n".to_vec()),
      truncated_reply(protocol),
      tool_reply(
        protocol,
        "write_file",
        json!({"path":"must-not-exist","content":"forbidden"}),
      ),
      text_reply(protocol, &"x".repeat(16 * 1024 + 1)),
    ] {
      let server = MockModelServer::start(vec![reply]).await.unwrap();
      let plan = engine(&server, protocol, policy())
        .decide("fixture request", None, CancellationToken::new())
        .await
        .unwrap();
      assert_fallback(&plan, FallbackReason::Unavailable);
      assert_eq!(
        server.requests().await.len(),
        1,
        "no hidden retry or tool follow-up"
      );
      assert!(
        !serde_json::to_string(&plan)
          .unwrap()
          .contains("synthetic private detail")
      );
    }
  }
}

#[tokio::test]
async fn capability_missing_and_threshold_boundary_are_explicit() {
  for protocol in StandardProtocol::ALL {
    let response = candidate("browser", &["browser_act"]);
    let server = MockModelServer::start(vec![text_reply(protocol, &response.to_string())])
      .await
      .unwrap();
    let mut limited = policy();
    limited.available_capabilities.remove(&Capability::Browser);
    let plan = engine(&server, protocol, limited)
      .decide("fixture", None, CancellationToken::new())
      .await
      .unwrap();
    assert_fallback(&plan, FallbackReason::MissingCapability);
    let mut response = candidate("code", &["write_file"]);
    response["confidence"] = json!(0.75);
    let server = MockModelServer::start(vec![text_reply(protocol, &response.to_string())])
      .await
      .unwrap();
    let plan = engine(&server, protocol, policy())
      .decide("fixture", None, CancellationToken::new())
      .await
      .unwrap();
    assert_eq!(plan.confidence, Some(0.75));
    assert_eq!(plan.allowed_tools, tools(&[Tool::WriteFile]));
  }
}

#[tokio::test]
async fn stalled_classifier_times_out_safely_and_cancellation_stops_routing() {
  for protocol in StandardProtocol::ALL {
    let server = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let mut bounded = policy();
    bounded.classifier_timeout = std::time::Duration::from_millis(100);
    let plan = engine(&server, protocol, bounded)
      .decide("fixture", None, CancellationToken::new())
      .await
      .unwrap();
    assert_fallback(&plan, FallbackReason::Timeout);
    let server = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let engine = engine(&server, protocol, policy());
    let cancellation = CancellationToken::new();
    let stop = cancellation.clone();
    let task = tokio::spawn(async move { engine.decide("fixture", None, stop).await });
    tokio::time::timeout(
      std::time::Duration::from_secs(2),
      server.wait_for_requests(1),
    )
    .await
    .unwrap();
    cancellation.cancel();
    assert_eq!(
      tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err(),
      DecisionError::Cancelled
    );
    assert_eq!(server.requests().await.len(), 1);
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
      support::engine(&server, protocol, policy())
        .decide("fixture", None, cancellation)
        .await
        .unwrap_err(),
      DecisionError::Cancelled
    );
    assert_eq!(
      server.requests().await.len(),
      1,
      "already cancelled request cannot invoke classifier"
    );
  }
}

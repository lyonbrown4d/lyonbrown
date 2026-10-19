use agent_test_support::{
  MockModelServer, ScriptedReply, StandardProtocol, text_reply, tool_reply,
};
use decision_core::*;
use decision_model::ModelDecisionBackend;
use model_provider::ModelConfig;
use serde_json::json;
use std::sync::Arc;

fn engine(server: &MockModelServer, protocol: StandardProtocol) -> DecisionEngine {
  DecisionEngine::new(
    Policy::default(),
    vec![Arc::new(ModelDecisionBackend::remote(ModelConfig {
      provider: protocol.provider().into(),
      model: "typed-fixture".into(),
      api_key: "synthetic-secret".into(),
      base_url: Some(server.base_url().into()),
      system_prompt: None,
      context_window: 16_384,
      max_output_tokens: 1024,
      supports_images: false,
    }))],
  )
  .unwrap()
}
#[tokio::test]
async fn every_protocol_returns_typed_judgments_and_rejects_bad_options_without_tools() {
  for protocol in StandardProtocol::ALL {
    let cases = [
      (
        Operation::Predicate(PredicateInput {
          context: "synthetic facts".into(),
          question: "Is condition satisfied?".into(),
        }),
        json!({"operation":"predicate","value":{"value":true,"confidence":0.95}}),
      ),
      (
        Operation::Choice(ChoiceInput {
          context: "synthetic facts".into(),
          options: vec!["first".into(), "second".into()],
        }),
        json!({"operation":"choice","value":{"selected":"second","distribution":[{"label":"first","score":0.05},{"label":"second","score":0.95}]}}),
      ),
      (
        Operation::Score(ScoreInput {
          context: "synthetic facts".into(),
          rubric: "quality".into(),
          minimum: 0.0,
          maximum: 10.0,
        }),
        json!({"operation":"score","value":{"value":8.5}}),
      ),
    ];
    for (operation, output) in cases {
      let reply = json!({"output":output,"confidence":0.95,"reason_codes":["FIXTURE"]}).to_string();
      let server = MockModelServer::start(vec![text_reply(protocol, &reply)])
        .await
        .unwrap();
      let result = engine(&server, protocol)
        .decide(
          &DecisionRequest::new("typed", operation),
          &DecisionCancellation::new(),
        )
        .await;
      assert_eq!(result.status, DecisionStatus::Accepted, "{result:?}");
      assert_eq!(serde_json::to_value(result.output).unwrap(), output);
      let requests = server.requests().await;
      assert_eq!(requests.len(), 1);
      assert!(
        requests[0]
          .body
          .get("tools")
          .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
      );
    }
    let bad=json!({"output":{"operation":"choice","value":{"selected":"unauthorized","distribution":[]}},"confidence":0.99,"reason_codes":[]}).to_string();
    let server = MockModelServer::start(vec![text_reply(protocol, &bad)])
      .await
      .unwrap();
    let result = engine(&server, protocol)
      .decide(
        &DecisionRequest::new(
          "bad",
          Operation::Choice(ChoiceInput {
            context: "data".into(),
            options: vec!["approved".into()],
          }),
        ),
        &DecisionCancellation::new(),
      )
      .await;
    assert_eq!(result.output, DecisionOutput::Unknown);
    assert_ne!(result.status, DecisionStatus::Accepted);
  }
}
#[tokio::test]
async fn typed_model_failure_and_cancellation_cannot_produce_a_judgment() {
  for protocol in StandardProtocol::ALL {
    let server = MockModelServer::start(vec![tool_reply(
      protocol,
      "write_file",
      json!({"path":"no"}),
    )])
    .await
    .unwrap();
    let request = DecisionRequest::new(
      "no-tools",
      Operation::Predicate(PredicateInput {
        context: "data".into(),
        question: "question".into(),
      }),
    );
    let result = engine(&server, protocol)
      .decide(&request, &DecisionCancellation::new())
      .await;
    assert_eq!(result.output, DecisionOutput::Unknown);
    let stalled = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let mut request = request.clone();
    request.deadline_ms = 30;
    let result = engine(&stalled, protocol)
      .decide(&request, &DecisionCancellation::new())
      .await;
    assert_eq!(result.output, DecisionOutput::Unknown);
    let cancellation = DecisionCancellation::new();
    cancellation.cancel();
    let result = engine(&stalled, protocol)
      .decide(&request, &cancellation)
      .await;
    assert_eq!(result.output, DecisionOutput::Unknown);
  }
}

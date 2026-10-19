mod support;
use decision_core::*;
use decision_model::ModelDecisionBackend;
use model_mock::Reply;
use serde_json::json;
use std::sync::Arc;

fn request() -> DecisionRequest {
  DecisionRequest::new(
    "synthetic",
    Operation::Predicate(PredicateInput {
      context: "synthetic facts".into(),
      question: "condition?".into(),
    }),
  )
}
fn decision(confidence: f64) -> Reply {
  Reply::Decision { text:json!({"output":{"operation":"predicate","value":{"value":true,"confidence":confidence}},"confidence":confidence,"reason_codes":["SYNTHETIC"]}).to_string(), delay_ms:0 }
}
#[tokio::test]
async fn structured_judgments_accept_only_valid_confident_advice_across_protocols() {
  for protocol in support::PROTOCOLS {
    let app = support::app(
      protocol,
      vec![
        decision(0.95),
        decision(0.1),
        support::text("not JSON"),
        Reply::Tool {
          name: "write_file".into(),
          arguments: json!({}),
          delay_ms: 0,
        },
        Reply::Error {
          status: 503,
          delay_ms: 0,
        },
        Reply::Truncated {
          text: "partial".into(),
        },
        Reply::Decision {text:json!({"output":{"operation":"choice","value":{"selected":"unauthorized","distribution":[]}},"confidence":0.99,"reason_codes":[]}).to_string(),delay_ms:0},
        Reply::Stall,
      ],
    )
    .await;
    let engine = DecisionEngine::new(
      Policy::default(),
      vec![Arc::new(ModelDecisionBackend::remote(support::config(
        &app, protocol,
      )))],
    )
    .unwrap();
    let first = engine
      .decide(&request(), &DecisionCancellation::new())
      .await;
    assert_eq!(first.status, DecisionStatus::Accepted);
    for _ in 0..5 {
      let failed = engine
        .decide(&request(), &DecisionCancellation::new())
        .await;
      assert_ne!(failed.status, DecisionStatus::Accepted);
      assert_eq!(failed.output, DecisionOutput::Unknown);
    }
    let choice = DecisionRequest::new(
      "bounded-choice",
      Operation::Choice(ChoiceInput {
        context: "synthetic".into(),
        options: vec!["authorized".into()],
      }),
    );
    assert_eq!(
      engine
        .decide(&choice, &DecisionCancellation::new())
        .await
        .output,
      DecisionOutput::Unknown
    );
    let mut timeout = request();
    timeout.deadline_ms = 30;
    assert_eq!(
      engine
        .decide(&timeout, &DecisionCancellation::new())
        .await
        .output,
      DecisionOutput::Unknown
    );
    let cancel = DecisionCancellation::new();
    cancel.cancel();
    assert_eq!(
      engine.decide(&request(), &cancel).await.output,
      DecisionOutput::Unknown
    );
  }
}

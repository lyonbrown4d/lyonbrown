mod support;
use decision_core::*;
use decision_model::ModelDecisionBackend;
use model_mock::Reply;
use serde_json::json;
use std::sync::Arc;

fn request() -> DecisionRequest {
  let mut request = DecisionRequest::new(
    "synthetic",
    Operation::Predicate(PredicateInput {
      context: "synthetic facts".into(),
      question: "condition?".into(),
    }),
  );
  // These fixtures check protocol behavior, not the constructor's 100 ms latency budget.
  request.deadline_ms = Policy::default().max_deadline_ms;
  request
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
    assert_eq!(
      first.status,
      DecisionStatus::Accepted,
      "{protocol:?}: {first:?}"
    );
    for expected_reason in [
      "LOW_CONFIDENCE",
      "BACKEND_UNAVAILABLE",
      "BACKEND_UNAVAILABLE",
      "BACKEND_UNAVAILABLE",
      "BACKEND_UNAVAILABLE",
    ] {
      let failed = engine
        .decide(&request(), &DecisionCancellation::new())
        .await;
      assert_ne!(
        failed.status,
        DecisionStatus::Accepted,
        "{protocol:?}: {failed:?}"
      );
      assert_eq!(
        failed.output,
        DecisionOutput::Unknown,
        "{protocol:?}: {failed:?}"
      );
      assert!(
        failed
          .provenance
          .reason_codes
          .iter()
          .any(|code| code == expected_reason),
        "{protocol:?}: expected {expected_reason}; {failed:?}"
      );
    }
    let mut choice = DecisionRequest::new(
      "bounded-choice",
      Operation::Choice(ChoiceInput {
        context: "synthetic".into(),
        options: vec!["authorized".into()],
      }),
    );
    choice.deadline_ms = Policy::default().max_deadline_ms;
    let rejected = engine.decide(&choice, &DecisionCancellation::new()).await;
    assert_eq!(
      rejected.output,
      DecisionOutput::Unknown,
      "{protocol:?}: {rejected:?}"
    );
    assert!(
      rejected
        .provenance
        .reason_codes
        .iter()
        .any(|code| code == "INVALID_BACKEND_OUTPUT"),
      "{protocol:?}: {rejected:?}"
    );
    let mut timeout = request();
    timeout.deadline_ms = 30;
    let timed_out = engine.decide(&timeout, &DecisionCancellation::new()).await;
    assert_eq!(
      timed_out.output,
      DecisionOutput::Unknown,
      "{protocol:?}: {timed_out:?}"
    );
    assert!(
      timed_out
        .provenance
        .reason_codes
        .iter()
        .any(|code| code == "BACKEND_TIMEOUT"),
      "{protocol:?}: {timed_out:?}"
    );
    let cancel = DecisionCancellation::new();
    cancel.cancel();
    let cancelled = engine.decide(&request(), &cancel).await;
    assert_eq!(
      cancelled.output,
      DecisionOutput::Unknown,
      "{protocol:?}: {cancelled:?}"
    );
    assert!(
      cancelled
        .provenance
        .reason_codes
        .iter()
        .any(|code| code == "CANCELLED"),
      "{protocol:?}: {cancelled:?}"
    );
  }
}

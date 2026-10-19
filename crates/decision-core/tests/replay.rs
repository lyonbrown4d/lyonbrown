use decision_core::*;
use serde::Deserialize;
#[derive(Deserialize)]
struct Case {
  id: String,
  text: String,
  expected_domain: Option<String>,
  expected_tier: Option<ModelTier>,
}

/// A small hand-authored regression corpus, not a production accuracy benchmark.
#[tokio::test]
async fn offline_zh_en_and_unknown_corpus_replays() {
  let engine = DecisionEngine::new(Policy::default(), vec![]).expect("valid policy");
  for corpus in [
    include_str!("../evaluations/zh.jsonl"),
    include_str!("../evaluations/en.jsonl"),
    include_str!("../evaluations/unknown.jsonl"),
  ] {
    for line in corpus.lines() {
      let case: Case = serde_json::from_str(line).expect("valid fixture");
      let request = DecisionRequest::new(
        case.id.clone(),
        Operation::RouteModel(RouteInput {
          text: case.text,
          ..RouteInput::default()
        }),
      );
      let result = engine.decide(&request, &DecisionCancellation::new()).await;
      match (case.expected_domain, case.expected_tier) {
        (Some(domain), Some(tier)) => {
          assert_eq!(result.status, DecisionStatus::Accepted, "{}", case.id);
          let DecisionOutput::RouteModel(advice) = result.output else {
            panic!("{} route", case.id)
          };
          assert_eq!(advice.task_domain, domain, "{}", case.id);
          assert_eq!(advice.recommended_tier, tier, "{}", case.id);
        }
        _ => {
          assert_eq!(result.status, DecisionStatus::Unknown, "{}", case.id);
          assert_eq!(result.output, DecisionOutput::Unknown);
        }
      }
    }
  }
}

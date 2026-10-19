mod support;
use decision_core::*;
use std::sync::Arc;
use support::*;

#[tokio::test]
async fn timeout_and_cancellation_do_not_turn_into_accepted_judgments() {
  let mut request = DecisionRequest::new(
    "bounded",
    Operation::Predicate(PredicateInput {
      context: "qzx".into(),
      question: "ok?".into(),
    }),
  );
  request.deadline_ms = 1;
  let mut pending = Stub::output(DecisionOutput::Predicate {
    value: true,
    confidence: 1.0,
  });
  pending.never_complete = true;
  let result = decide(&request, vec![Arc::new(pending)]).await;
  assert_eq!(result.status, DecisionStatus::Unknown);
  assert!(
    result
      .provenance
      .reason_codes
      .iter()
      .any(|v| v == "BACKEND_TIMEOUT")
  );

  let cancellation = DecisionCancellation::new();
  let mut cancelling = Stub::output(DecisionOutput::Predicate {
    value: true,
    confidence: 1.0,
  });
  cancelling.never_complete = true;
  cancelling.cancel = Some(cancellation.clone());
  request.deadline_ms = 5000;
  let engine =
    DecisionEngine::new(Policy::default(), vec![Arc::new(cancelling)]).expect("valid policy");
  let result = engine.decide(&request, &cancellation).await;
  assert_eq!(result.status, DecisionStatus::Unknown);
  assert_eq!(result.output, DecisionOutput::Unknown);
  assert!(
    result
      .provenance
      .reason_codes
      .iter()
      .any(|v| v == "CANCELLED")
  );
}

#[tokio::test]
async fn policy_replacement_is_validated_atomic_and_version_pinned() {
  let engine = DecisionEngine::new(Policy::default(), vec![]).expect("valid policy");
  let held_snapshot = engine.snapshot();
  let invalid = Policy {
    minimum_confidence: f64::NAN,
    ..Policy::default()
  };
  assert!(engine.replace_policy(invalid).is_err());
  assert_eq!(engine.snapshot().policy().revision, "rules-v1");
  let mut replacement = Policy {
    revision: "rules-v2".into(),
    ..Policy::default()
  };
  replacement.domains[0].tier = ModelTier::Fast;
  engine
    .replace_policy(replacement.clone())
    .expect("new revision");
  assert_eq!(held_snapshot.policy().revision, "rules-v1");
  assert_eq!(engine.snapshot().policy().revision, "rules-v2");
  replacement.domains[0].tier = ModelTier::Advanced;
  assert!(engine.replace_policy(replacement).is_err());
  let mut request = route("Rust", vec![]);
  request.policy_revision = Some("rules-v1".into());
  assert_eq!(
    engine
      .decide(&request, &DecisionCancellation::new())
      .await
      .status,
    DecisionStatus::Denied
  );
  request.policy_revision = Some("rules-v2".into());
  let result = engine.decide(&request, &DecisionCancellation::new()).await;
  assert_eq!(result.provenance.policy_revision, "rules-v2");
  let DecisionOutput::RouteModel(advice) = result.output else {
    panic!("route")
  };
  assert_eq!(advice.recommended_tier, ModelTier::Fast);
}

#[tokio::test]
async fn valid_backend_after_invalid_backend_is_explicitly_degraded() {
  let request = DecisionRequest::new(
    "chain",
    Operation::Score(ScoreInput {
      context: "qzx".into(),
      rubric: "quality".into(),
      minimum: 0.0,
      maximum: 1.0,
    }),
  );
  let result = decide(
    &request,
    vec![
      Arc::new(Stub::output(DecisionOutput::Score { value: 2.0 })),
      Arc::new(Stub::output(DecisionOutput::Score { value: 0.5 })),
    ],
  )
  .await;
  assert_eq!(result.status, DecisionStatus::Degraded);
  assert_eq!(result.output, DecisionOutput::Score { value: 0.5 });
  assert!(
    result
      .provenance
      .reason_codes
      .iter()
      .any(|v| v == "INVALID_BACKEND_OUTPUT")
  );
}

#[tokio::test]
async fn invalid_input_and_oversized_output_are_bounded() {
  let policy = Policy {
    max_input_bytes: 256,
    ..Policy::default()
  };
  let engine = DecisionEngine::new(policy, vec![]).expect("policy");
  let request = route(&"Rust".repeat(100), vec![]);
  assert_eq!(
    engine
      .decide(&request, &DecisionCancellation::new())
      .await
      .status,
    DecisionStatus::Denied
  );
  let policy = Policy {
    max_output_bytes: 20,
    ..Policy::default()
  };
  let engine = DecisionEngine::new(
    policy,
    vec![Arc::new(Stub::output(DecisionOutput::Score { value: 0.5 }))],
  )
  .expect("policy");
  let request = DecisionRequest::new(
    "large",
    Operation::Score(ScoreInput {
      context: "qzx".into(),
      rubric: "q".into(),
      minimum: 0.0,
      maximum: 1.0,
    }),
  );
  let result = engine.decide(&request, &DecisionCancellation::new()).await;
  assert_eq!(result.status, DecisionStatus::Unknown);
}

#[tokio::test]
async fn backend_abstention_preserves_unknown_provenance_without_claiming_acceptance() {
  let request = DecisionRequest::new(
    "abstain",
    Operation::Predicate(PredicateInput {
      context: "qzx".into(),
      question: "ok?".into(),
    }),
  );
  let result = decide(
    &request,
    vec![Arc::new(Stub::output(DecisionOutput::Unknown))],
  )
  .await;
  assert_eq!(result.status, DecisionStatus::Unknown);
  assert_eq!(result.output, DecisionOutput::Unknown);
  assert_eq!(result.provenance.method, "test:unknown");
  assert_eq!(
    result.provenance.index_revision.as_deref(),
    Some("index-v2")
  );
  assert!(
    result
      .provenance
      .reason_codes
      .iter()
      .any(|code| code == "TEST_EVIDENCE")
  );
  assert!(
    !result
      .provenance
      .reason_codes
      .iter()
      .any(|code| code == "INVALID_BACKEND_OUTPUT")
  );
}

mod support;
use decision_core::*;
use std::sync::Arc;
use support::*;

#[tokio::test]
async fn typed_backend_operations_preserve_output_and_versions() {
  let pairs = vec![
    (
      Operation::Classify(ClassifyInput {
        text: "qzx".into(),
        labels: vec!["custom".into()],
      }),
      DecisionOutput::Classify {
        label: "custom".into(),
        alternatives: vec![],
      },
    ),
    (
      Operation::Predicate(PredicateInput {
        context: "evidence".into(),
        question: "passes?".into(),
      }),
      DecisionOutput::Predicate {
        value: false,
        confidence: 0.9,
      },
    ),
    (
      Operation::Choice(ChoiceInput {
        context: "evidence".into(),
        options: vec!["a".into(), "b".into()],
      }),
      DecisionOutput::Choice {
        selected: "b".into(),
        distribution: vec![WeightedLabel {
          label: "b".into(),
          score: 1.0,
        }],
      },
    ),
    (
      Operation::Score(ScoreInput {
        context: "evidence".into(),
        rubric: "quality".into(),
        minimum: -1.0,
        maximum: 1.0,
      }),
      DecisionOutput::Score { value: 0.5 },
    ),
    (
      Operation::RecommendTools(ToolInput {
        text: "qzx".into(),
        authorized_tools: vec![ToolCandidate {
          id: "read".into(),
          description: "inspect files".into(),
        }],
        top_k: 1,
      }),
      DecisionOutput::RecommendTools {
        tools: vec![ToolRecommendation {
          id: "read".into(),
          score: 0.8,
        }],
      },
    ),
  ];
  for (operation, output) in pairs {
    let request = DecisionRequest::new("typed", operation);
    let result = decide(&request, vec![Arc::new(Stub::output(output.clone()))]).await;
    assert_eq!(result.status, DecisionStatus::Accepted);
    assert_eq!(result.output, output);
    assert_eq!(
      result.provenance.backend_version.as_deref(),
      Some("backend-v1")
    );
    assert_eq!(
      result.provenance.index_revision.as_deref(),
      Some("index-v2")
    );
  }
}

#[tokio::test]
async fn invalid_typed_results_remain_unknown() {
  let cases = vec![
    (
      Operation::Classify(ClassifyInput {
        text: "qzx".into(),
        labels: vec!["a".into()],
      }),
      DecisionOutput::Classify {
        label: "invented".into(),
        alternatives: vec![],
      },
    ),
    (
      Operation::Choice(ChoiceInput {
        context: "qzx".into(),
        options: vec!["a".into()],
      }),
      DecisionOutput::Choice {
        selected: "a".into(),
        distribution: vec![WeightedLabel {
          label: "a".into(),
          score: 0.2,
        }],
      },
    ),
    (
      Operation::Score(ScoreInput {
        context: "qzx".into(),
        rubric: "quality".into(),
        minimum: 0.0,
        maximum: 1.0,
      }),
      DecisionOutput::Score { value: 2.0 },
    ),
    (
      Operation::Predicate(PredicateInput {
        context: "qzx".into(),
        question: "ok?".into(),
      }),
      DecisionOutput::Predicate {
        value: true,
        confidence: f64::NAN,
      },
    ),
    (
      Operation::RecommendTools(ToolInput {
        text: "qzx".into(),
        authorized_tools: vec![],
        top_k: 1,
      }),
      DecisionOutput::RecommendTools {
        tools: vec![ToolRecommendation {
          id: "unauthorized".into(),
          score: 1.0,
        }],
      },
    ),
    (
      Operation::Predicate(PredicateInput {
        context: "qzx".into(),
        question: "ok?".into(),
      }),
      DecisionOutput::Score { value: 1.0 },
    ),
  ];
  for (operation, output) in cases {
    let result = decide(
      &DecisionRequest::new("invalid", operation),
      vec![Arc::new(Stub::output(output))],
    )
    .await;
    assert_eq!(result.status, DecisionStatus::Unknown);
    assert_eq!(result.output, DecisionOutput::Unknown);
    assert!(
      result
        .provenance
        .reason_codes
        .iter()
        .any(|r| r == "INVALID_BACKEND_OUTPUT")
    );
  }
}

#[tokio::test]
async fn facts_context_advice_and_authorized_tools_work_offline() {
  let request = DecisionRequest::new(
    "facts",
    Operation::ContextAdvice(ContextInput {
      used_tokens: 70,
      capacity_tokens: 100,
      reserved_output_tokens: 10,
    }),
  );
  let result = decide(&request, vec![]).await;
  assert_eq!(
    result.output,
    DecisionOutput::ContextAdvice {
      should_compact: true,
      available_tokens: 20
    }
  );
  let request = DecisionRequest::new(
    "tools",
    Operation::RecommendTools(ToolInput {
      text: "inspect files".into(),
      authorized_tools: vec![ToolCandidate {
        id: "read".into(),
        description: "inspect files".into(),
      }],
      top_k: 1,
    }),
  );
  assert_eq!(
    decide(&request, vec![]).await.output,
    DecisionOutput::RecommendTools {
      tools: vec![ToolRecommendation {
        id: "read".into(),
        score: 1.0
      }]
    }
  );
}

#[tokio::test]
async fn missing_or_low_confidence_judgment_is_unknown() {
  let request = DecisionRequest::new(
    "unknown",
    Operation::Predicate(PredicateInput {
      context: "untrusted".into(),
      question: "safe?".into(),
    }),
  );
  assert_eq!(
    decide(&request, vec![]).await.status,
    DecisionStatus::Unknown
  );
  let mut backend = Stub::output(DecisionOutput::Predicate {
    value: true,
    confidence: 1.0,
  });
  backend.confidence = 0.1;
  let result = decide(&request, vec![Arc::new(backend)]).await;
  assert_eq!(result.status, DecisionStatus::Unknown);
  assert_eq!(result.output, DecisionOutput::Unknown);
}

use decision_core::{
  BackendError, ClassifyInput, DecisionBackend, DecisionCancellation, DecisionOutput,
  DecisionRequest, Operation, Policy, PolicySnapshot, ToolCandidate, ToolInput,
};
use decision_semantic::{
  EmbeddingDescriptor, EmbeddingProvider, NegativeExample, SemanticBackend, SemanticConfig,
  SemanticExample,
};
use serde::Deserialize;
use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

struct Synthetic;
impl EmbeddingProvider for Synthetic {
  fn descriptor(&self) -> EmbeddingDescriptor {
    EmbeddingDescriptor {
      model_revision: "synthetic-v1".into(),
      preprocessing_revision: "exact-fixture-v1".into(),
      dimensions: 3,
      local: true,
    }
  }
  fn embed(&self, text: &str) -> Result<Vec<f32>, BackendError> {
    Ok(match text {
      "coding example" | "请检查锁竞争" | "Inspect lock contention" => vec![1.0, 0.0, 0.0],
      "chat example" | "给朋友打个招呼" | "Greet my friend" => vec![0.0, 1.0, 0.0],
      "混合问候与锁竞争" | "Mixed greeting and lock contention" => vec![1.0, 1.0, 0.0],
      "invalid dimensions" => vec![1.0],
      "nonfinite" => vec![f32::NAN, 0.0, 0.0],
      "zero" => vec![0.0; 3],
      "provider failure" => return Err(BackendError::Unavailable),
      _ => vec![0.0, 0.0, 1.0],
    })
  }
}
fn config() -> SemanticConfig {
  SemanticConfig {
    index_revision: "synthetic-index-v1".into(),
    config_revision: "thresholds-v1".into(),
    minimum_score: 0.7,
    minimum_margin: 0.1,
    negative_penalty: 0.4,
    max_concurrent_inference: 1,
    max_input_bytes: 4096,
    examples: vec![
      SemanticExample {
        label: "coding".into(),
        text: "coding example".into(),
      },
      SemanticExample {
        label: "chat".into(),
        text: "chat example".into(),
      },
    ],
    hard_negatives: vec![],
  }
}
fn classify(text: &str) -> DecisionRequest {
  DecisionRequest::new(
    "semantic-test",
    Operation::Classify(ClassifyInput {
      text: text.into(),
      labels: vec!["coding".into(), "chat".into()],
    }),
  )
}
fn policy() -> PolicySnapshot {
  PolicySnapshot::new(Policy::default()).expect("policy")
}

#[derive(Deserialize)]
struct Replay {
  text: String,
  expected: Option<String>,
}
#[tokio::test]
async fn fixed_bilingual_synthetic_replay_preserves_unknown_and_versions() {
  let backend = SemanticBackend::load(config(), Arc::new(Synthetic))
    .await
    .expect("load corpus");
  for line in include_str!("fixtures/replay.jsonl").lines() {
    let case: Replay = serde_json::from_str(line).expect("fixture");
    let result = backend
      .evaluate(
        &classify(&case.text),
        &policy(),
        &DecisionCancellation::new(),
      )
      .await
      .expect("decision");
    match (&result.output, case.expected) {
      (DecisionOutput::Classify { label, .. }, Some(expected)) => assert_eq!(label, &expected),
      (DecisionOutput::Unknown, None) => {}
      (actual, expected) => panic!("replay mismatch: {actual:?}, expected {expected:?}"),
    }
  }
  let descriptor = backend.descriptor();
  assert_eq!(
    descriptor.index_revision.as_deref(),
    Some("synthetic-index-v1")
  );
  assert!(descriptor.version.contains("contract-sha256="));
  assert!(descriptor.version.len() <= 128);
  let mut replacement = config();
  replacement.config_revision = "thresholds-v2".into();
  let updated = SemanticBackend::load(replacement, Arc::new(Synthetic))
    .await
    .expect("replacement");
  assert_ne!(descriptor.version, updated.descriptor().version);
}
#[tokio::test]
async fn malformed_embedding_and_provider_failure_do_not_become_a_label() {
  let backend = SemanticBackend::load(config(), Arc::new(Synthetic))
    .await
    .expect("load");
  for text in [
    "invalid dimensions",
    "nonfinite",
    "zero",
    "provider failure",
  ] {
    assert!(
      backend
        .evaluate(&classify(text), &policy(), &DecisionCancellation::new())
        .await
        .is_err()
    );
  }
  let mut malformed = config();
  malformed.minimum_margin = f64::NAN;
  assert!(
    SemanticBackend::load(malformed, Arc::new(Synthetic))
      .await
      .is_err()
  );
}
#[tokio::test]
async fn hard_negative_penalty_and_authorized_tool_recall_are_enforced() {
  let mut corpus = config();
  corpus.hard_negatives.push(NegativeExample {
    label: "coding".into(),
    text: "coding example".into(),
  });
  let negative = SemanticBackend::load(corpus, Arc::new(Synthetic))
    .await
    .expect("negative corpus");
  let result = negative
    .evaluate(
      &classify("请检查锁竞争"),
      &policy(),
      &DecisionCancellation::new(),
    )
    .await
    .expect("decision");
  assert!(matches!(result.output, DecisionOutput::Unknown));
  let backend = SemanticBackend::load(config(), Arc::new(Synthetic))
    .await
    .expect("load");
  let request = DecisionRequest::new(
    "tools",
    Operation::RecommendTools(ToolInput {
      text: "Inspect lock contention".into(),
      top_k: 2,
      authorized_tools: vec![
        ToolCandidate {
          id: "read".into(),
          description: "coding example".into(),
        },
        ToolCandidate {
          id: "greet".into(),
          description: "chat example".into(),
        },
      ],
    }),
  );
  let result = backend
    .evaluate(&request, &policy(), &DecisionCancellation::new())
    .await
    .expect("tools");
  let DecisionOutput::RecommendTools { tools } = result.output else {
    panic!("tools expected")
  };
  assert_eq!(tools.len(), 1);
  assert_eq!(tools[0].id, "read");
}

struct Gated {
  entered: Arc<AtomicBool>,
  release: Arc<AtomicBool>,
  second_entered: Arc<AtomicBool>,
}
impl EmbeddingProvider for Gated {
  fn descriptor(&self) -> EmbeddingDescriptor {
    Synthetic.descriptor()
  }
  fn embed(&self, text: &str) -> Result<Vec<f32>, BackendError> {
    if text == "first query" {
      self.entered.store(true, Ordering::SeqCst);
      while !self.release.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(1));
      }
    }
    if text == "second query" {
      self.second_entered.store(true, Ordering::SeqCst);
    }
    Synthetic.embed("coding example")
  }
}
#[tokio::test]
async fn cancelled_worker_retains_concurrency_slot_until_native_work_finishes() {
  let entered = Arc::new(AtomicBool::new(false));
  let release = Arc::new(AtomicBool::new(false));
  let second_entered = Arc::new(AtomicBool::new(false));
  let backend = SemanticBackend::load(
    config(),
    Arc::new(Gated {
      entered: entered.clone(),
      release: release.clone(),
      second_entered: second_entered.clone(),
    }),
  )
  .await
  .expect("load");
  let cancellation = DecisionCancellation::new();
  let worker_backend = backend.clone();
  let token = cancellation.clone();
  let first = tokio::spawn(async move {
    worker_backend
      .evaluate(&classify("first query"), &policy(), &token)
      .await
  });
  tokio::time::timeout(Duration::from_secs(2), async {
    while !entered.load(Ordering::SeqCst) {
      tokio::task::yield_now().await;
    }
  })
  .await
  .expect("first worker entered");
  cancellation.cancel();
  assert!(matches!(
    first.await.expect("join"),
    Err(BackendError::Cancelled)
  ));
  let next_backend = backend.clone();
  let second = tokio::spawn(async move {
    next_backend
      .evaluate(
        &classify("second query"),
        &policy(),
        &DecisionCancellation::new(),
      )
      .await
  });
  tokio::time::sleep(Duration::from_millis(20)).await;
  let prematurely_entered = second_entered.load(Ordering::SeqCst);
  // Always release the native worker, including on assertion failure.
  release.store(true, Ordering::SeqCst);
  second.await.expect("second join").expect("second result");
  assert!(!prematurely_entered);
  assert!(second_entered.load(Ordering::SeqCst));
}

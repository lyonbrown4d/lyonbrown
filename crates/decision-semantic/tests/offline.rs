#![cfg(feature = "semantic-local")]
use decision_core::{
  ClassifyInput, DecisionBackend, DecisionCancellation, DecisionOutput, DecisionRequest, Operation,
  Policy, PolicySnapshot,
};
use decision_semantic::{
  EmbeddingProvider, LocalFastEmbed, OfflineEmbeddingConfig, SemanticBackend, SemanticConfig,
  SemanticExample,
};
use std::sync::Arc;

const EXAMPLES: &[(&str, &str)] = &[
  ("coding", "Find and fix a bug in this Rust program"),
  ("coding", "请找出这段程序的逻辑错误"),
  ("chat", "Hello, how are you today?"),
  ("chat", "嗨，今天过得怎么样"),
];
const CALIBRATION: &[(&str, &str)] = &[
  ("coding", "Investigate why my application crashes"),
  ("coding", "帮我排查程序为什么崩溃"),
  ("chat", "Good morning! How's your day?"),
  ("chat", "早上好，最近怎么样"),
];
const NEGATIVES: &[&str] = &[
  "Photosynthesis converts sunlight into chemical energy.",
  "一只白鹭站在河边的芦苇丛中",
];
const REPLAY: &[(&str, Option<&str>)] = &[
  (
    "Debug a function that returns wrong results",
    Some("coding"),
  ),
  ("修复这个函数中的错误", Some("coding")),
  ("Hi, nice to meet you", Some("chat")),
  ("你好，很高兴见到你", Some("chat")),
  ("The isotope has a half-life of 5730 years.", None),
  ("太平洋海沟的深度是多少", None),
];

/// A small offline adapter acceptance, with a disjoint calibration and replay set.
/// It is not a multilingual quality benchmark. No model hub or runtime downloads are enabled.
#[tokio::test]
#[ignore = "requires LYONBROWN_TEST_EMBEDDING_CONFIG with caller-provisioned, licensed, checksum-pinned local assets"]
async fn caller_supplied_real_model_replays_bilingual_queries_with_measured_thresholds() {
  let manifest =
    std::env::var("LYONBROWN_TEST_EMBEDDING_CONFIG").expect("explicit local asset manifest");
  let config: OfflineEmbeddingConfig =
    serde_json::from_slice(&std::fs::read(manifest).expect("read manifest"))
      .expect("parse manifest");
  let dimensions = config.dimensions;
  let provider = Arc::new(
    LocalFastEmbed::load(config)
      .await
      .expect("offline model load"),
  );
  let worker_provider = provider.clone();
  // Inference, including fixture calibration, stays outside Tokio core I/O workers.
  let (minimum_score, minimum_margin) = tokio::task::spawn_blocking(move || {
    let examples: Vec<_> = EXAMPLES
      .iter()
      .map(|(label, text)| {
        let vector = worker_provider.embed(text).expect("real corpus inference");
        assert_eq!(vector.len(), dimensions);
        assert!(vector.iter().all(|v| v.is_finite()));
        (*label, vector)
      })
      .collect();
    let rank = |text: &str| {
      let vector = worker_provider
        .embed(text)
        .expect("real calibration inference");
      let coding = examples
        .iter()
        .filter(|(label, _)| *label == "coding")
        .map(|(_, reference)| {
          f64::from(fastembed::similarity::cosine_similarity(&vector, reference))
        })
        .fold(-1.0_f64, f64::max);
      let chat = examples
        .iter()
        .filter(|(label, _)| *label == "chat")
        .map(|(_, reference)| {
          f64::from(fastembed::similarity::cosine_similarity(&vector, reference))
        })
        .fold(-1.0_f64, f64::max);
      if coding >= chat {
        ("coding", coding, coding - chat)
      } else {
        ("chat", chat, chat - coding)
      }
    };
    let mut minimum_known = 1.0_f64;
    let mut minimum_known_margin = 1.0_f64;
    for (expected, text) in CALIBRATION {
      let (label, score, margin) = rank(text);
      eprintln!(
        "calibration expected={expected} selected={label} score={score:.6} margin={margin:.6}"
      );
      assert_eq!(*expected, label, "fixed calibration label");
      minimum_known = minimum_known.min(score);
      minimum_known_margin = minimum_known_margin.min(margin);
    }
    let maximum_unknown = NEGATIVES
      .iter()
      .map(|text| rank(text).1)
      .fold(-1.0_f64, f64::max);
    assert!(
      minimum_known > maximum_unknown,
      "fixed calibration corpus is not score-separable"
    );
    assert!(minimum_known_margin > 0.0);
    (
      (minimum_known + maximum_unknown) / 2.0,
      minimum_known_margin / 2.0,
    )
  })
  .await
  .expect("calibration worker");
  eprintln!("measured fixture thresholds score={minimum_score:.6} margin={minimum_margin:.6}");
  let backend = SemanticBackend::load(
    SemanticConfig {
      index_revision: "offline-adapter-replay-v1".into(),
      config_revision: "fixture-calibrated-v1".into(),
      minimum_score,
      minimum_margin,
      negative_penalty: 0.0,
      max_concurrent_inference: 1,
      max_input_bytes: 4096,
      examples: EXAMPLES
        .iter()
        .map(|(label, text)| SemanticExample {
          label: (*label).into(),
          text: (*text).into(),
        })
        .collect(),
      hard_negatives: vec![],
    },
    provider,
  )
  .await
  .expect("real semantic index");
  let policy = PolicySnapshot::new(Policy {
    minimum_confidence: minimum_score,
    ..Policy::default()
  })
  .expect("policy");
  for (index, (text, expected)) in REPLAY.iter().enumerate() {
    let request = DecisionRequest::new(
      format!("offline-replay-{index}"),
      Operation::Classify(ClassifyInput {
        text: (*text).into(),
        labels: vec!["coding".into(), "chat".into()],
      }),
    );
    let result = backend
      .evaluate(&request, &policy, &DecisionCancellation::new())
      .await
      .expect("real semantic replay");
    eprintln!(
      "replay index={index} similarity={:.6} output={:?}",
      result.confidence, result.output
    );
    match (result.output, expected) {
      (DecisionOutput::Classify { label, .. }, Some(expected)) => {
        assert_eq!(label.as_str(), *expected)
      }
      (DecisionOutput::Unknown, None) => {}
      (actual, expected) => {
        panic!("fixed replay {index}: expected {expected:?}, actual {actual:?}")
      }
    }
  }
}

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
  Fast,
  Balanced,
  Advanced,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Complexity {
  Low,
  Medium,
  High,
}
impl From<ModelTier> for Complexity {
  fn from(tier: ModelTier) -> Self {
    match tier {
      ModelTier::Fast => Self::Low,
      ModelTier::Balanced => Self::Medium,
      ModelTier::Advanced => Self::High,
    }
  }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Constraints {
  pub local_only: bool,
  /// Empty means no additional tier restriction; candidate authorization remains caller owned.
  pub allowed_tiers: BTreeSet<ModelTier>,
  /// None means no additional key restriction; Some(empty) permits no models.
  pub allowed_model_keys: Option<BTreeSet<String>>,
  pub required_capabilities: BTreeSet<String>,
  pub input_tokens: Option<u64>,
  pub max_estimated_cost_microusd: Option<u64>,
}

/// Caller-authorized, current model metadata. Unknown capacity/cost cannot satisfy a supplied limit.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCandidate {
  pub key: String,
  pub tier: ModelTier,
  pub local: bool,
  pub capabilities: BTreeSet<String>,
  pub context_tokens: Option<u64>,
  pub estimated_cost_microusd: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RouteInput {
  pub text: String,
  pub candidates: Vec<ModelCandidate>,
  pub explicit_model: Option<String>,
  pub pinned_model: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifyInput {
  pub text: String,
  pub labels: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PredicateInput {
  pub context: String,
  pub question: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceInput {
  pub context: String,
  pub options: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreInput {
  pub context: String,
  pub rubric: String,
  pub minimum: f64,
  pub maximum: f64,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCandidate {
  pub id: String,
  pub description: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInput {
  pub text: String,
  pub authorized_tools: Vec<ToolCandidate>,
  pub top_k: usize,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextInput {
  pub used_tokens: u64,
  pub capacity_tokens: u64,
  pub reserved_output_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
  tag = "operation",
  content = "input",
  rename_all = "snake_case",
  deny_unknown_fields
)]
pub enum Operation {
  RouteModel(RouteInput),
  Classify(ClassifyInput),
  Predicate(PredicateInput),
  Choice(ChoiceInput),
  Score(ScoreInput),
  RecommendTools(ToolInput),
  ContextAdvice(ContextInput),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRequest {
  pub request_id: String,
  pub operation: Operation,
  pub constraints: Constraints,
  /// Exact revision pin, if desired. A mismatch denies the request.
  pub policy_revision: Option<String>,
  pub deadline_ms: u64,
}
impl DecisionRequest {
  pub fn new(request_id: impl Into<String>, operation: Operation) -> Self {
    Self {
      request_id: request_id.into(),
      operation,
      constraints: Constraints::default(),
      policy_revision: None,
      deadline_ms: 100,
    }
  }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
  Accepted,
  Unknown,
  Degraded,
  Denied,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RouteAdvice {
  pub complexity: Complexity,
  pub task_domain: String,
  pub recommended_tier: ModelTier,
  pub required_capabilities: BTreeSet<String>,
  pub candidate_model_keys: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedLabel {
  pub label: String,
  pub score: f64,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRecommendation {
  pub id: String,
  pub score: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
  tag = "operation",
  content = "value",
  rename_all = "snake_case",
  deny_unknown_fields
)]
pub enum DecisionOutput {
  RouteModel(RouteAdvice),
  Classify {
    label: String,
    alternatives: Vec<WeightedLabel>,
  },
  Predicate {
    value: bool,
    confidence: f64,
  },
  Choice {
    selected: String,
    distribution: Vec<WeightedLabel>,
  },
  Score {
    value: f64,
  },
  RecommendTools {
    tools: Vec<ToolRecommendation>,
  },
  ContextAdvice {
    should_compact: bool,
    available_tokens: u64,
  },
  Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
  pub policy_id: String,
  pub policy_revision: String,
  pub method: String,
  pub backend_version: Option<String>,
  pub index_revision: Option<String>,
  pub reason_codes: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionResult {
  pub request_id: String,
  pub status: DecisionStatus,
  pub output: DecisionOutput,
  pub provenance: Provenance,
  pub elapsed_ms: u64,
}

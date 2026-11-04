use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StepState {
  Pending,
  Running,
  Blocked,
  Completed,
  Skipped,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
  Model,
  Host,
  User,
}
/// Constructed by the daemon from its authenticated session and persisted run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanContext {
  pub session_id: String,
  pub run_id: String,
  pub owner_epoch: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDraft {
  pub id: Uuid,
  pub title: String,
  pub dependencies: Vec<Uuid>,
  pub acceptance: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanDraft {
  pub title: String,
  pub steps: Vec<StepDraft>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationRecord {
  pub id: Uuid,
  pub step_id: Uuid,
  /// Stable reference to host-observed execution evidence, never a model claim.
  pub evidence_ref: String,
  pub acceptance: Vec<String>,
  pub passed: bool,
  pub summary: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
  pub definition: StepDraft,
  pub state: StepState,
  /// An untrusted model report is retained separately from host verification.
  pub reported_result: String,
  pub verification_refs: Vec<Uuid>,
  pub user_override: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
  pub id: Uuid,
  pub session_id: String,
  pub run_id: String,
  pub owner_epoch: u64,
  pub revision: u64,
  pub title: String,
  pub steps: Vec<PlanStep>,
  pub verifications: Vec<VerificationRecord>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanChange {
  Create {
    draft: PlanDraft,
  },
  Revise {
    draft: PlanDraft,
  },
  VerifyStep {
    step_id: Uuid,
    operation_id: String,
  },
  UpdateStep {
    step_id: Uuid,
    state: StepState,
    reported_result: String,
    verification_refs: Vec<Uuid>,
    user_override: Option<String>,
  },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanMutation {
  pub request_id: Uuid,
  pub plan_id: Uuid,
  /// Zero only for create. Every successful mutation increments this value.
  pub expected_revision: u64,
  pub change: PlanChange,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanPage {
  pub plans: Vec<Plan>,
  pub next_after: Option<Uuid>,
}

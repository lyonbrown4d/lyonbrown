use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
  pub id: String,
  pub title: String,
  pub workspace: String,
  pub owner_node_id: String,
  pub owner_epoch: u64,
  pub created_at_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Run {
  pub id: String,
  pub session_id: String,
  pub status: String,
  pub prompt: String,
  pub result: String,
  pub error: String,
  pub created_at_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
  pub cursor: u64,
  pub run_id: String,
  pub kind: String,
  pub payload_json: String,
  pub created_at_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Approval {
  pub id: String,
  pub run_id: String,
  pub tool: String,
  pub arguments_json: String,
  pub status: String,
  pub expires_at_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskEnvelope {
  pub task_id: String,
  pub parent_run_id: String,
  pub session_id: String,
  pub owner_node_id: String,
  pub owner_epoch: u64,
  pub attempt_id: String,
  pub instructions: String,
  pub capabilities: Vec<String>,
  pub workspace: String,
  pub idempotency_key: String,
  pub deadline_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
  pub envelope: TaskEnvelope,
  pub status: String,
  pub attempt_status: String,
  pub result: String,
  pub error: String,
  pub cursor: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerTrust {
  #[serde(default)]
  pub certificate_pem: String,
  #[serde(default)]
  pub noise_public_key: String,
  pub node_id: String,
  pub fingerprint: String,
  pub capabilities: Vec<String>,
  pub revoked: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactRef {
  pub digest: String,
  pub size: u64,
  pub mime: String,
  pub node_id: String,
}

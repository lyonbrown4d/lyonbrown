use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Each invocation has a caller-owned durable identity; executors never retry it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRequest {
  pub action_id: String,
  pub executable: PathBuf,
  pub arguments: Vec<String>,
  /// Relative to the authorized workspace. Use `.` for its root.
  pub cwd: PathBuf,
  pub timeout_ms: u64,
  /// Combined stdout and stderr bytes, before UTF-8 decoding.
  pub output_limit_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCommand {
  pub executable: PathBuf,
  pub cwd: PathBuf,
}

/// The daemon must persist approval and execution intent here before returning true.
/// Approval must bind to the exact action, executable, arguments and resolved cwd supplied.
#[async_trait]
pub trait CommandAuthorization: Send + Sync {
  async fn authorize(
    &self,
    request: &CommandRequest,
    resolved: &ResolvedCommand,
  ) -> anyhow::Result<bool>;
}

pub struct DenyCommandAuthorization;
#[async_trait]
impl CommandAuthorization for DenyCommandAuthorization {
  async fn authorize(&self, _: &CommandRequest, _: &ResolvedCommand) -> anyhow::Result<bool> {
    Ok(false)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
  Succeeded,
  /// The child exited unsuccessfully; callers must not automatically replay it.
  Failed,
  Rejected,
  Denied,
  CancelledBeforeStart,
  /// Dispatch occurred and side effects may have happened. Never replay automatically.
  Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStopReason {
  InvalidRequest,
  Unauthorized,
  AuthorizationFailed,
  Cancelled,
  Deadline,
  OutputLimit,
  SpawnFailed,
  ProcessIo,
  CleanupFailed,
  UnsupportedPlatform,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandOutcome {
  pub action_id: String,
  pub status: CommandStatus,
  pub reason: Option<CommandStopReason>,
  pub exit_code: Option<i32>,
  pub stdout: String,
  pub stderr: String,
  pub output_truncated: bool,
}
impl CommandOutcome {
  pub(super) fn stopped(
    request: &CommandRequest,
    status: CommandStatus,
    reason: CommandStopReason,
  ) -> Self {
    Self {
      action_id: request.action_id.clone(),
      status,
      reason: Some(reason),
      exit_code: None,
      stdout: String::new(),
      stderr: String::new(),
      output_truncated: false,
    }
  }
}

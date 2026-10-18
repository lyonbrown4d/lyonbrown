use anyhow::{Context, Result, ensure};
use serde_json::Value;

pub(super) fn validate_summary(summary: &Value) -> Result<()> {
  let metadata = summary
    .as_object()
    .context("operation summary must be metadata object")?;
  ensure!(
    serde_json::to_vec(summary)?.len() <= 4096,
    "operation summary exceeds limit"
  );
  for (key, value) in metadata {
    ensure!(
      matches!(
        key.as_str(),
        "reason"
          | "status"
          | "exit_code"
          | "pid"
          | "timed_out"
          | "cancelled"
          | "output_truncated"
          | "duration_ms"
          | "stdout_bytes"
          | "stderr_bytes"
          | "argument_count"
      ),
      "unsupported operation metadata key"
    );
    ensure!(
      match (key.as_str(), value) {
        ("reason", Value::String(reason)) => matches!(
          reason.as_str(),
          "invalid_request"
            | "unauthorized"
            | "authorization_failed"
            | "cancelled"
            | "deadline"
            | "output_limit"
            | "spawn_failed"
            | "process_io"
            | "cleanup_failed"
            | "unsupported_platform"
            | "daemon_restarted"
        ),
        ("status", Value::String(status)) => matches!(
          status.as_str(),
          "completed"
            | "succeeded"
            | "failed"
            | "uncertain"
            | "cancelled"
            | "timed_out"
            | "rejected"
            | "denied"
            | "cancelled_before_start"
        ),
        ("timed_out" | "cancelled" | "output_truncated", Value::Bool(_)) => true,
        ("exit_code" | "reason", Value::Null) => true,
        ("exit_code", Value::Number(number)) => number.is_i64(),
        (
          "pid" | "duration_ms" | "stdout_bytes" | "stderr_bytes" | "argument_count",
          Value::Number(number),
        ) => number.is_u64(),
        _ => false,
      },
      "invalid operation metadata value"
    );
  }
  Ok(())
}

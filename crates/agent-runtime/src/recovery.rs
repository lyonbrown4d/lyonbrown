use std::sync::Arc;

use rpi_harness::{runtime::SessionRuntime, session::Session};

use crate::{Result, RuntimeError, SessionPersistence, persistence::DurableSession};

/// Read-only evidence. Inspection never constructs a Harness or resumes an operation.
#[derive(Clone, Debug, Default)]
pub struct RecoveryReport {
  pub pending_tools: Vec<String>,
  pub uncertainty: Option<String>,
}

pub async fn inspect_recovery(
  session_id: &str,
  owner_epoch: u64,
  persistence: Arc<dyn SessionPersistence>,
) -> Result<RecoveryReport> {
  let storage = DurableSession::open(session_id, owner_epoch, persistence).await?;
  let session = Session::new(Arc::new(storage), None);
  let report = SessionRuntime::new(session.clone())
    .recover_lane("main")
    .await
    .map_err(|error| RuntimeError::Persistence(error.to_string()))?;
  let mut evidence = RecoveryReport {
    pending_tools: report
      .tool_frames
      .iter()
      .map(|tool| format!("{} ({})", tool.tool_name, tool.tool_call_id))
      .collect(),
    uncertainty: report
      .is_corrupt()
      .then(|| "Harness operation records are corrupt".into()),
  };
  let entries = session
    .find_entries(&Default::default())
    .await
    .map_err(|error| RuntimeError::Persistence(error.to_string()))?;
  for entry in entries {
    if let rpi_harness::session::types::Entry::Message(message) = entry
      && let rpi_agent::message::AgentMessage::ToolResult(result) = message.message
      && result
        .details
        .as_ref()
        .is_some_and(|details| details[crate::tools::UNCERTAIN_DETAIL].as_bool() == Some(true))
    {
      evidence
        .pending_tools
        .push(format!("{} ({})", result.tool_name, result.tool_call_id));
    }
  }
  evidence.pending_tools.sort();
  evidence.pending_tools.dedup();
  if !evidence.pending_tools.is_empty() && evidence.uncertainty.is_none() {
    evidence.uncertainty =
      Some("tool effects require human verification; never replay automatically".into());
  }
  Ok(evidence)
}

use crate::{RunOutcome, RuntimeResult};
use rpi_harness::agent_harness::{HarnessRunOutcome, RunResult};
pub(super) fn convert_result(result: RunResult) -> RuntimeResult {
  let (outcome, message) = match result.outcome {
    HarnessRunOutcome::Completed { final_message, .. } => {
      (RunOutcome::Completed, Some(final_message))
    }
    HarnessRunOutcome::Aborted { final_message, .. } => {
      (RunOutcome::Cancelled, Some(final_message))
    }
    HarnessRunOutcome::Failed {
      error,
      final_message,
      ..
    } => (
      RunOutcome::Failed {
        message: error.message,
      },
      final_message,
    ),
    HarnessRunOutcome::Suspended { final_entry_id, .. } => {
      (RunOutcome::Suspended { id: final_entry_id }, None)
    }
  };
  let usage = message
    .as_ref()
    .map(|message| crate::TokenUsage {
      input_tokens: message.usage.input.max(0) as u64,
      output_tokens: message.usage.output.max(0) as u64,
      cache_read_tokens: message.usage.cache_read.max(0) as u64,
      cache_write_tokens: message.usage.cache_write.max(0) as u64,
      total_tokens: message.usage.total_tokens.max(0) as u64,
    })
    .unwrap_or_default();
  let text = message
    .map(|message| {
      message
        .content
        .iter()
        .filter_map(|content| match content {
          rpi_ai::types::Content::Text(text) => Some(text.text.as_str()),
          _ => None,
        })
        .collect::<String>()
    })
    .unwrap_or_default();
  RuntimeResult {
    run_id: result.run_id,
    outcome,
    text,
    usage,
  }
}

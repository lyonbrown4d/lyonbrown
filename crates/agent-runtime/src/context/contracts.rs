use crate::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Deliberately contains no transcript, summary text, credentials or provider SDK types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStatus {
  pub revision: u64,
  pub message_count: usize,
  pub compaction_count: usize,
  pub has_summary: bool,
  pub can_undo: bool,
  /// Rig's heuristic estimate of the next request, including system prompt and tools.
  /// This is not a provider measurement or tokenizer count.
  pub estimated_input_tokens: u64,
  pub estimate_includes_tools: bool,
  pub context_window: u32,
  pub reserved_output_tokens: u32,
  pub summary_request_pending: bool,
  pub summary_pipeline_available: bool,
}

/// The two text messages produced by Rig's Summarizer, before provider IO.
/// This value must never be logged or placed in a client model.
pub struct SummaryInput {
  pub system_prompt: String,
  pub transcript: String,
}

/// Optional additional host policy applied before canonical privacy preparation.
/// It can narrow or deny a request. Runtime privacy protection always runs after
/// this policy and cannot be bypassed by omitting or replacing this guard.
#[async_trait]
pub trait SummaryRequestGuard: Send + Sync {
  async fn prepare(
    &self,
    input: SummaryInput,
    cancellation: CancellationToken,
  ) -> Result<SummaryInput>;
}

mod archive;
mod contracts;
mod inspection;
mod maintenance;
mod operations;
mod summary_io;
mod tasks;

pub(crate) use archive::ContextArchive;
pub use contracts::{ContextStatus, SummaryInput, SummaryRequestGuard};
pub use inspection::{inspect_context, inspect_summary, undo_context};
pub use maintenance::cancel_unsent_summary;

use crate::{Result, Runtime, RuntimeError};
use rig_core::{catalog::ModelSpec, providers::registry::ProviderId};
use rig_memory::CompactionPolicy;

impl Runtime {
  pub fn with_summary_guard(mut self, guard: std::sync::Arc<dyn SummaryRequestGuard>) -> Self {
    self.summary_guard = Some(guard);
    self
  }

  pub(crate) fn context_policy(&self) -> CompactionPolicy {
    let mut policy = CompactionPolicy::default();
    policy.summarizer.limits.reserve = u64::from(self.config.max_output_tokens)
      .saturating_add(u64::from(self.config.context_window / 8).min(rig_memory::OUTPUT_MARGIN));
    policy.summarizer.limits.summary_tokens = u64::from(self.config.max_output_tokens).min(8192);
    policy
  }

  fn context_spec(&self) -> Result<ModelSpec> {
    let mut spec = match self.model.spec() {
      Some(spec) => spec.clone(),
      None => ModelSpec::new(
        ProviderId::declare(self.model.name()).map_err(|_| {
          RuntimeError::Configuration("model summary capabilities unavailable".into())
        })?,
        self.config.model.clone(),
      ),
    };
    // Explicit host limits may narrow catalog facts; never widen a known limit.
    spec.context_window = Some(
      spec
        .context_window
        .map_or(self.config.context_window, |limit| {
          limit.min(self.config.context_window)
        }),
    );
    spec.max_output_tokens = Some(
      spec
        .max_output_tokens
        .map_or(self.config.max_output_tokens, |limit| {
          limit.min(self.config.max_output_tokens)
        }),
    );
    Ok(spec)
  }

  fn context_estimate_for(
    &self,
    messages: Vec<rig_core::message::Message>,
    preamble: Option<&str>,
  ) -> Result<u64> {
    let history = rig_core::conversation::Conversation::from_messages(messages);
    let tools = self.tool_definitions()?;
    Ok(rig_memory::next_request_tokens(
      None,
      None,
      &rig_memory::Prepared {
        model: &self.config.model,
        prompt: preamble,
        tools: &tools,
        history: &history,
      },
    ))
  }

  pub async fn context_status(&self) -> Result<ContextStatus> {
    let state = self.session.state.lock().await;
    let preamble = state
      .active
      .as_ref()
      .map_or(self.config.system_prompt.as_deref(), |active| {
        active.spec.preamble.as_deref()
      });
    let estimated = self.context_estimate_for(state.context.messages()?, preamble)?;
    let mut status = inspection::status(&state, &self.config, estimated, true)?;
    status.estimate_includes_tools = true;
    status.can_undo &= !self
      .session
      .failed
      .load(std::sync::atomic::Ordering::Acquire);
    Ok(status)
  }
}

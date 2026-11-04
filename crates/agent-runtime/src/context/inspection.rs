use super::ContextArchive;
use crate::{ContextStatus, Result, RuntimeConfig, SessionPersistence, ToolDefinition};
use rig_core::{conversation::Conversation, message::Message};
use rig_memory::{Prepared, next_request_tokens};
use std::sync::Arc;

/// Read-only inspection also works for a fenced/uncertain session. It never
/// constructs a provider, invokes a guard, replays IO or changes the owner epoch.
/// No summary pipeline is installed in this detached inspection.
pub async fn inspect_context(
  config: &RuntimeConfig,
  persistence: Arc<dyn SessionPersistence>,
  tools: &[ToolDefinition],
) -> Result<ContextStatus> {
  let session =
    crate::persistence::DurableSession::open(&config.session_id, config.owner_epoch, persistence)
      .await?;
  let state = session.state.lock().await;
  let tools = tools
    .iter()
    .map(|tool| {
      Ok(Arc::new(rig_core::completion::ToolDefinition {
        name: rig_core::message::ToolName::new(tool.name.clone())
          .map_err(crate::persistence::rig_error)?,
        description: tool.description.clone(),
        parameters: tool.parameters.clone(),
        annotations: Default::default(),
      }))
    })
    .collect::<Result<Vec<_>>>()?;
  let mut effective = config.clone();
  if let Some(active) = &state.active {
    effective.system_prompt = active.spec.preamble.clone();
  }
  let estimated = estimate(&effective, state.context.messages()?, &tools);
  status(&state, config, estimated, false)
}

pub(super) fn estimate(
  config: &RuntimeConfig,
  messages: Vec<Message>,
  tools: &[Arc<rig_core::completion::ToolDefinition>],
) -> u64 {
  let history = Conversation::from_messages(messages);
  next_request_tokens(
    None,
    None,
    &Prepared {
      model: &config.model,
      prompt: config.system_prompt.as_deref(),
      tools,
      history: &history,
    },
  )
}

pub(super) fn status(
  state: &crate::persistence::Snapshot,
  config: &RuntimeConfig,
  estimated_input_tokens: u64,
  summary_pipeline_available: bool,
) -> Result<ContextStatus> {
  let context: &ContextArchive = &state.context;
  let conversation = context.conversation()?;
  let compaction_count = rig_memory::Compacted::of(&conversation).count();
  Ok(ContextStatus {
    revision: context.revision,
    message_count: context.messages()?.len(),
    compaction_count,
    has_summary: !rig_memory::summary_state(&conversation).summary.is_empty(),
    can_undo: compaction_count > 0
      && state.active.is_none()
      && crate::persistence::ensure_snapshot_certain(state).is_ok(),
    estimated_input_tokens,
    estimate_includes_tools: false,
    context_window: config.context_window,
    reserved_output_tokens: config.max_output_tokens,
    summary_request_pending: context.pending.is_some(),
    summary_pipeline_available,
  })
}

/// Explicit bounded summary inspection; never returns the underlying transcript.
pub async fn inspect_summary(
  config: &RuntimeConfig,
  persistence: Arc<dyn SessionPersistence>,
) -> Result<(u64, String, bool)> {
  let session =
    crate::persistence::DurableSession::open(&config.session_id, config.owner_epoch, persistence)
      .await?;
  let state = session.state.lock().await;
  let conversation = state.context.conversation()?;
  let summary = rig_memory::summary_state(&conversation).summary;
  let truncated = summary.len() > 131_072;
  let mut end = summary.len().min(131_072);
  while !summary.is_char_boundary(end) {
    end -= 1;
  }
  Ok((state.context.revision, summary[..end].to_owned(), truncated))
}

/// Undo is local persisted context editing and does not require a configured model.
pub async fn undo_context(
  config: &RuntimeConfig,
  persistence: Arc<dyn SessionPersistence>,
  expected_revision: u64,
) -> Result<ContextStatus> {
  let session =
    crate::persistence::DurableSession::open(&config.session_id, config.owner_epoch, persistence)
      .await?;
  session.ensure_certain().await?;
  session
    .update("context_compaction_undone", |state| {
      if state.active.is_some() || state.context.revision != expected_revision {
        return Err(crate::RuntimeError::Configuration(
          "context is active or revision changed".into(),
        ));
      }
      state.context.undo()?;
      Ok((
        (),
        serde_json::json!({"revision":state.context.revision}),
        vec![],
      ))
    })
    .await?;
  let state = session.state.lock().await;
  status(
    &state,
    config,
    estimate(config, state.context.messages()?, &[]),
    false,
  )
}

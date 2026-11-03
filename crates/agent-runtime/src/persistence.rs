//! The store commits the Rig state, application facts and owner fence together.
use crate::{Result, RuntimeError, SessionMutation, SessionPersistence, TokenUsage};
use agent_core::events::{AgentEvent, QueueKind, QueueStatus};
use rig_agent::run::{AgentRun, PreparedRequest, RunSpec};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
  collections::BTreeMap,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
  pub version: u32,
  pub session_id: String,
  pub context: crate::context::ContextArchive,
  pub active: Option<ActiveRun>,
  pub pending_tools: BTreeMap<String, ToolIntent>,
  pub queue: Vec<QueuedInput>,
  pub uncertainty: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ActiveRun {
  pub id: String,
  pub run: AgentRun,
  pub spec: RunSpec,
  pub prepared: Option<PreparedRequest>,
  /// Persisted before any provider IO; a restart may never replay it.
  pub in_flight: Option<String>,
  pub usage: TokenUsage,
  pub segment: u64,
  pub context_cursor: usize,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ToolIntent {
  pub call_id: String,
  pub name: String,
  pub arguments: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct QueuedInput {
  pub id: String,
  pub request_id: String,
  pub kind: QueueKind,
  pub prompt: String,
  pub status: QueueStatus,
}
impl QueuedInput {
  pub fn event(&self, status: QueueStatus) -> AgentEvent {
    AgentEvent::QueueUpdated {
      entry_id: self.id.clone(),
      kind: self.kind,
      status,
      prompt: matches!(status, QueueStatus::Pending).then(|| self.prompt.clone()),
      request_id: Some(self.request_id.clone()),
    }
  }
}
pub(crate) struct DurableSession {
  pub state: Mutex<Snapshot>,
  persistence: Arc<dyn SessionPersistence>,
  id: String,
  epoch: u64,
  pub failed: AtomicBool,
  pub stopped: CancellationToken,
}
impl DurableSession {
  pub async fn open(
    id: &str,
    epoch: u64,
    persistence: Arc<dyn SessionPersistence>,
  ) -> Result<Self> {
    let state = match persistence.load(id).await? {
      Some(bytes) => {
        if bytes.len() > crate::budget::SNAPSHOT_BYTES {
          return Err(RuntimeError::Persistence(
            "session snapshot exceeds host byte limit".into(),
          ));
        }
        let value: Snapshot = serde_json::from_slice(&bytes)
          .map_err(|error| RuntimeError::Persistence(error.to_string()))?;
        if value.version != 1 || value.session_id != id {
          return Err(RuntimeError::Persistence(
            "invalid Rig session identity/version".into(),
          ));
        }
        crate::budget::validate_snapshot(&value)?;
        value
      }
      None => Snapshot {
        version: 1,
        session_id: id.into(),
        context: Default::default(),
        active: None,
        pending_tools: BTreeMap::new(),
        queue: vec![],
        uncertainty: None,
      },
    };
    Ok(Self {
      state: Mutex::new(state),
      persistence,
      id: id.into(),
      epoch,
      failed: AtomicBool::new(false),
      stopped: CancellationToken::new(),
    })
  }
  pub async fn ensure_certain(&self) -> Result<()> {
    if self.failed.load(Ordering::Acquire) {
      return Err(RuntimeError::Uncertain(
        "session persistence requires reconciliation".into(),
      ));
    }
    let state = self.state.lock().await;
    ensure_snapshot_certain(&state)
  }
  /// Copy-on-commit retains the last confirmed state when the host refuses a write.
  /// Every commit passes the epoch to the relational store's transactional fence.
  pub async fn update<T>(
    &self,
    kind: &str,
    f: impl FnOnce(&mut Snapshot) -> Result<(T, Value, Vec<AgentEvent>)>,
  ) -> Result<T> {
    if self.failed.load(Ordering::Acquire) {
      return Err(RuntimeError::Uncertain("session writer is fenced".into()));
    }
    let mut current = self.state.lock().await;
    // A concurrent writer may have failed while this caller waited for the lock.
    if self.failed.load(Ordering::Acquire) || self.stopped.is_cancelled() {
      return Err(RuntimeError::Uncertain("session writer is fenced".into()));
    }
    let mut next = current.clone();
    let (output, payload, events) = f(&mut next)?;
    let (next, bytes) = crate::budget::snapshot(next).await.map_err(|error| {
      if current
        .active
        .as_ref()
        .is_some_and(|active| active.in_flight.is_some())
        || !current.pending_tools.is_empty()
        || current
          .context
          .pending
          .as_ref()
          .is_some_and(|intent| intent.sending)
      {
        RuntimeError::Uncertain(
          "confirmed state retained after host byte limit; IO intent requires reconciliation"
            .into(),
        )
      } else {
        error
      }
    })?;
    if let Err(error) = self
      .persistence
      .commit(
        &self.id,
        self.epoch,
        bytes,
        SessionMutation {
          kind: kind.into(),
          payload,
          events,
        },
      )
      .await
    {
      self.failed.store(true, Ordering::Release);
      self.stopped.cancel();
      return Err(RuntimeError::Uncertain(format!(
        "session commit failed; writer fenced: {error}"
      )));
    }
    *current = next;
    Ok(output)
  }
  pub async fn uncertain(&self, message: String) -> Result<()> {
    self
      .update("execution_uncertain", |state| {
        state.uncertainty = Some(message.clone());
        Ok(((), json!({"outcome":"uncertain"}), vec![]))
      })
      .await?;
    Err(RuntimeError::Uncertain(message))
  }
}
pub(crate) fn ensure_snapshot_certain(state: &Snapshot) -> Result<()> {
  if let Some(message) = &state.uncertainty {
    return Err(RuntimeError::Uncertain(message.clone()));
  }
  if state.context.pending.is_some() {
    return Err(RuntimeError::Uncertain(
      "summary request outcome was not confirmed; never replay".into(),
    ));
  }
  if !state.pending_tools.is_empty() {
    return Err(RuntimeError::Uncertain(
      "tool outcome was not durably confirmed; never replay".into(),
    ));
  }
  if state
    .active
    .as_ref()
    .is_some_and(|active| active.in_flight.is_some())
  {
    return Err(RuntimeError::Uncertain(
      "provider request outcome was not confirmed; never replay".into(),
    ));
  }
  Ok(())
}
pub(crate) fn active(state: &mut Snapshot) -> Result<&mut ActiveRun> {
  state
    .active
    .as_mut()
    .ok_or_else(|| RuntimeError::Execution("no active Rig run".into()))
}
pub(crate) fn rig_error(error: impl std::fmt::Display) -> RuntimeError {
  RuntimeError::Execution(error.to_string())
}

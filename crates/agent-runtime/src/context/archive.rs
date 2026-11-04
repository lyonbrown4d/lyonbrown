//! Rig's own log is nested in the host snapshot so splice history, undo data,
//! state/event/outbox and the owner epoch are committed by one host transaction.
use crate::{Result, RuntimeError, persistence::rig_error};
use rig_core::{conversation::Conversation, message::Message};
use rig_memory::{Files, MemoryStore, Store};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const LOG: &str = "context";

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContextArchive {
  /// The original task remains verbatim, outside the summarizable conversation.
  pub pinned: Option<Message>,
  pub tasks: Vec<super::tasks::TaskEntry>,
  pub log: Vec<u8>,
  pub files: BTreeMap<String, Vec<u8>>,
  pub revision: u64,
  pub pending: Option<SummaryIntent>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SummaryIntent {
  pub id: String,
  pub revision: u64,
  pub sending: bool,
}

impl ContextArchive {
  pub fn store(&self) -> Result<MemoryStore> {
    let store = MemoryStore::default();
    if !self.log.is_empty() {
      store.replace_log(LOG, 0, &self.log).map_err(rig_error)?;
    }
    for (name, bytes) in &self.files {
      store.write_file(name, bytes).map_err(rig_error)?;
    }
    Ok(store)
  }

  pub fn conversation(&self) -> Result<Conversation> {
    rig_memory::load(&self.store()?, LOG).map_err(rig_error)
  }

  pub fn save(&mut self, conversation: &mut Conversation) -> Result<()> {
    crate::budget::check(self, crate::budget::CONTEXT_BYTES)?;
    let store = self.store()?;
    rig_memory::save(&store, LOG, conversation).map_err(rig_error)?;
    self.log = store.read_log(LOG).map_err(rig_error)?;
    self.files = store
      .files()
      .map_err(rig_error)?
      .into_iter()
      .map(|name| store.read_file(&name).map(|bytes| (name, bytes)))
      .collect::<std::io::Result<_>>()
      .map_err(rig_error)?;
    self.revision = self.revision.saturating_add(1);
    crate::budget::check(self, crate::budget::CONTEXT_BYTES)
  }

  pub fn protect(&mut self, message: &Message) {
    if self.pinned.is_some()
      && let Some(message) = super::tasks::text(message)
    {
      self
        .tasks
        .push(super::tasks::TaskEntry { id: None, message });
    }
  }
  pub fn messages(&self) -> Result<Vec<Message>> {
    Ok(
      self
        .pinned
        .iter()
        .cloned()
        .chain(super::tasks::projection(&self.conversation()?, &self.tasks))
        .collect(),
    )
  }
  pub fn summarizable(&self) -> Result<Conversation> {
    super::tasks::summarizable(&self.conversation()?, &self.tasks)
  }
  /// Only called at a complete transcript boundary. The cursor follows the
  /// AgentRun source, which remains Rig-owned even after its request projection
  /// has been compacted. No native/tool operation is inferred from summary text.
  pub fn append(&mut self, messages: &[Message], cursor: &mut usize) -> Result<()> {
    if *cursor > messages.len() {
      return Err(RuntimeError::Execution(
        "context source cursor moved backwards".into(),
      ));
    }
    let mut conversation = self.conversation()?;
    let mut added = messages.iter().skip(*cursor).cloned();
    if self.pinned.is_none() {
      self.pinned = added.next();
    }
    for message in added {
      let task_text = super::tasks::text(&message);
      let id = conversation.push(message);
      if let Some(task_text) = task_text
        && let Some(task) = self
          .tasks
          .iter_mut()
          .find(|task| task.id.is_none() && task.message == task_text)
      {
        task.id = Some(id);
      }
    }
    conversation.validate().map_err(rig_error)?;
    if messages.len() > *cursor {
      self.save(&mut conversation)?;
    }
    *cursor = messages.len();
    Ok(())
  }

  pub fn undo(&mut self) -> Result<()> {
    let store = self.store()?;
    let mut conversation = rig_memory::load(&store, LOG).map_err(rig_error)?;
    rig_memory::undo_summary(&mut conversation, &store, LOG, rig_memory::summary_author())
      .map_err(rig_error)?;
    self.save(&mut conversation)
  }
}

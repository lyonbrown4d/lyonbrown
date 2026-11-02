//! Client-only input recall. Returned strings must be placed in the editor, never dispatched.
mod history;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use history::{
  InputHistory, MAX_HISTORY_BYTES, MAX_HISTORY_ENTRIES, MAX_HISTORY_SESSIONS, MAX_PROMPT_BYTES,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Navigation {
  index: Option<usize>,
  draft: String,
}

/// Persistable history plus transient recall cursors. The connection scope contains no credentials.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionState {
  connection_scope: String,
  pub history: InputHistory,
  #[serde(skip)]
  navigation: BTreeMap<String, Navigation>,
}

impl InteractionState {
  /// Clear history before using another daemon/account identity. Reconnects to the same scope retain it.
  pub fn bind_connection(&mut self, scope: &str) {
    if self.connection_scope != scope {
      self.history = InputHistory::default();
      self.connection_scope = scope.into();
      self.navigation.clear();
    }
  }

  /// Refuse persisted history from another daemon/account; transient drafts are never imported.
  pub fn import_history(&mut self, stored: Self) -> bool {
    if self.connection_scope.is_empty() || self.connection_scope != stored.connection_scope {
      return false;
    }
    self.history = stored.history;
    self.navigation.clear();
    true
  }

  pub fn record_acknowledged(&mut self, session: &str, prompt: &str) -> bool {
    self.reset_navigation(session);
    self.history.record_acknowledged(session, prompt)
  }

  pub fn reset_navigation(&mut self, session: &str) {
    self.navigation.remove(session);
  }

  pub fn previous(&mut self, session: &str, draft: &str) -> Option<String> {
    let entries = self.history.entries(session);
    if entries.is_empty() {
      return None;
    }
    // Keep transient cursors bounded too; they do not contain acknowledged history.
    if !self.navigation.contains_key(session) && self.navigation.len() >= MAX_HISTORY_SESSIONS {
      self.navigation.clear();
    }
    let nav = self.navigation.entry(session.into()).or_default();
    let index = match nav.index {
      Some(index) => index.saturating_sub(1),
      None => {
        if draft.len() > MAX_PROMPT_BYTES {
          return None;
        }
        nav.draft = draft.into();
        entries.len() - 1
      }
    };
    nav.index = Some(index);
    entries.get(index).cloned()
  }

  /// After the newest history row, restore the editor's original draft.
  pub fn next(&mut self, session: &str) -> Option<String> {
    let nav = self.navigation.get_mut(session)?;
    let index = nav.index? + 1;
    if let Some(prompt) = self.history.entries(session).get(index) {
      nav.index = Some(index);
      Some(prompt.clone())
    } else {
      nav.index = None;
      Some(std::mem::take(&mut nav.draft))
    }
  }

  pub fn select(&mut self, session: &str, index: usize) -> Option<String> {
    let prompt = self.history.entries(session).get(index)?.clone();
    self.reset_navigation(session);
    Some(prompt)
  }
}

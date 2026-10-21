use agent_protocol::v1::{Approval, Run, Session};
use std::collections::BTreeMap;

/// Active chat presentation state. Execution and persistence remain daemon-owned.
pub(crate) struct DesktopViewModel {
  pub(crate) sessions: Vec<Session>,
  pub(crate) selected: Option<String>,
  pub(crate) runs: Vec<Run>,
  pub(crate) active_run: Option<String>,
  pub(crate) cursor: u64,
  pub(crate) busy: bool,
  pub(crate) error: Option<String>,
  pub(crate) status: String,
  pub(crate) conversation: Vec<crate::message::ChatMessage>,
  pub(crate) approvals: Vec<Approval>,
}

impl Default for DesktopViewModel {
  fn default() -> Self {
    Self {
      sessions: Vec::new(),
      selected: None,
      runs: Vec::new(),
      active_run: None,
      cursor: 0,
      busy: false,
      error: None,
      status: "Disconnected".into(),
      conversation: Vec::new(),
      approvals: Vec::new(),
    }
  }
}

impl DesktopViewModel {
  /// Group daemon-provided workspace paths without merging distinct path spellings.
  pub(crate) fn session_groups(&self) -> BTreeMap<&str, Vec<&Session>> {
    let mut groups = BTreeMap::<&str, Vec<&Session>>::new();
    for session in &self.sessions {
      groups
        .entry(session.workspace.as_str())
        .or_default()
        .push(session);
    }
    groups
  }
}

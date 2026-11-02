use crate::{recovery::RecoveryState, shell::Desktop};
use agent_protocol::v1::{Approval, Run, Session};
use std::collections::BTreeMap;

/// Presentation state of exactly one chat tab. Execution and persistence remain
/// daemon-owned.
///
/// Invariant: `Desktop.vm` is the state of the active tab only. Every inactive
/// tab owns its own value inside `Navigation::inactive` (`ChatState::vm`), and
/// `ChatState::swap_with` is the single place where the two are exchanged.
pub(crate) struct DesktopViewModel {
  pub(crate) selected: Option<String>,
  pub(crate) runs: Vec<Run>,
  pub(crate) active_run: Option<String>,
  pub(crate) cursor: u64,
  pub(crate) busy: bool,
  pub(crate) error: Option<String>,
  pub(crate) status: String,
  pub(crate) conversation: Vec<crate::message::ChatMessage>,
  pub(crate) approvals: Vec<Approval>,
  pub(crate) recovery: Option<RecoveryState>,
}

impl Default for DesktopViewModel {
  fn default() -> Self {
    Self {
      selected: None,
      runs: Vec::new(),
      active_run: None,
      cursor: 0,
      busy: false,
      error: None,
      status: "Disconnected".into(),
      conversation: Vec::new(),
      approvals: Vec::new(),
      recovery: None,
    }
  }
}

impl Desktop {
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

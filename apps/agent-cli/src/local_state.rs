use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::ContextPanel;

/// Presentation state only. Daemon state, credentials and live execution data are excluded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientState {
  #[serde(default)]
  pub input_attempt: Option<crate::run_input_ui::InputAttempt>,
  #[serde(default)]
  pub interaction: crate::interaction::InteractionState,
  pub selected_session: Option<String>,
  pub drafts: BTreeMap<String, String>,
  pub unbound_draft: String,
  pub panel: PersistedPanel,
  pub transcript_scroll: usize,
  pub context_scroll: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedPanel {
  #[default]
  Closed,
  Help,
  Sessions,
  Tasks,
  Computer,
  Approval,
  Drafts,
  Activity,
  Inputs,
}

impl From<ContextPanel> for PersistedPanel {
  fn from(panel: ContextPanel) -> Self {
    match panel {
      ContextPanel::Closed => Self::Closed,
      ContextPanel::Help => Self::Help,
      ContextPanel::Sessions => Self::Sessions,
      ContextPanel::Tasks => Self::Tasks,
      ContextPanel::Computer => Self::Computer,
      ContextPanel::Approval => Self::Approval,
      ContextPanel::Drafts => Self::Drafts,
      ContextPanel::Activity => Self::Activity,
      ContextPanel::Inputs => Self::Inputs,
    }
  }
}

impl From<PersistedPanel> for ContextPanel {
  fn from(panel: PersistedPanel) -> Self {
    match panel {
      PersistedPanel::Closed => Self::Closed,
      PersistedPanel::Help => Self::Help,
      PersistedPanel::Sessions => Self::Sessions,
      PersistedPanel::Tasks => Self::Tasks,
      PersistedPanel::Computer => Self::Computer,
      PersistedPanel::Approval => Self::Approval,
      PersistedPanel::Drafts => Self::Drafts,
      PersistedPanel::Activity => Self::Activity,
      PersistedPanel::Inputs => Self::Inputs,
    }
  }
}

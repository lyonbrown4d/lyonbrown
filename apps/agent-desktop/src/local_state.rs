//! Presentation-only state restored after an authenticated daemon handshake.
use gpui_kit::component::{ThemeMode, dock::DockAreaState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DesktopState {
  pub tabs: Vec<TabState>,
  pub active_tab: u64,
  pub show_sidebar: bool,
  pub show_workbench: bool,
  pub dock: DockAreaState,
  pub layout_sizes: Vec<f32>,
  pub theme: ThemeMode,
  pub editor_drafts: Vec<EditorDraftState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct TabState {
  pub id: u64,
  pub session_id: Option<String>,
  pub unbound_from: Option<String>,
  pub composer: String,
  pub workspace: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct EditorDraftState {
  pub session_id: Option<String>,
  pub path: String,
  pub base_revision: String,
  pub base_content: String,
  pub text: String,
}

impl DesktopState {
  pub(crate) fn validate(&self) -> anyhow::Result<()> {
    anyhow::ensure!(
      !self.tabs.is_empty() && self.tabs.len() <= 64,
      "invalid desktop tab count"
    );
    let mut ids = std::collections::HashSet::with_capacity(self.tabs.len());
    for tab in &self.tabs {
      anyhow::ensure!(
        tab.id != 0 && tab.id != u64::MAX && ids.insert(tab.id),
        "invalid or duplicate desktop tab ID"
      );
      anyhow::ensure!(
        tab.composer.len() <= 256 * 1024,
        "desktop composer draft is too large"
      );
      anyhow::ensure!(
        tab.workspace.len() <= 4096,
        "desktop workspace path is too large"
      );
      for session_id in [tab.session_id.as_ref(), tab.unbound_from.as_ref()]
        .into_iter()
        .flatten()
      {
        anyhow::ensure!(
          !session_id.is_empty() && session_id.len() <= 256,
          "invalid desktop session ID"
        );
      }
    }
    anyhow::ensure!(
      ids.contains(&self.active_tab),
      "active desktop tab is missing"
    );
    anyhow::ensure!(
      self.layout_sizes.len() <= 16,
      "too many desktop layout sizes"
    );
    anyhow::ensure!(
      self
        .layout_sizes
        .iter()
        .all(|size| size.is_finite() && *size >= 0.0 && *size <= 16_384.0),
      "invalid desktop layout size"
    );
    anyhow::ensure!(
      self.editor_drafts.len() <= 256,
      "too many desktop editor drafts"
    );
    for draft in &self.editor_drafts {
      anyhow::ensure!(
        !draft.path.is_empty() && draft.path.len() <= 4096,
        "invalid editor path"
      );
      anyhow::ensure!(
        draft.base_revision.len() <= 4096
          && draft.base_content.len() <= 1024 * 1024
          && draft.text.len() <= 1024 * 1024,
        "editor draft is too large"
      );
      if let Some(session_id) = &draft.session_id {
        anyhow::ensure!(
          !session_id.is_empty() && session_id.len() <= 256,
          "invalid editor session ID"
        );
      }
    }
    Ok(())
  }
}

use crate::local_state::DesktopState;
use crate::persistence::worker::Command;
use crate::shell::Desktop;
use gpui_kit::component::{ActiveTheme, ThemeMode};
use gpui_kit::*;

/// Cheap projection of the persisted inputs that no observed entity reports.
///
/// Composer and workspace text, workbench layout, dock state, layout sizes and
/// editor drafts are reported by `cx.observe` subscriptions. Tabs, session
/// binding, sidebar/workbench visibility and the theme change through paths
/// that only redraw the window, so the frame they change in compares this key
/// instead of rebuilding [`DesktopState`], whose drafts are up to 1 MiB each.
#[derive(Default, PartialEq)]
pub(crate) struct SnapshotKey {
  tabs: Vec<u64>,
  active_tab: u64,
  unbound: Vec<(u64, String)>,
  session: Option<String>,
  show_sidebar: bool,
  show_workbench: bool,
  theme: ThemeMode,
}

impl SnapshotKey {
  fn capture(desktop: &Desktop, cx: &App) -> Self {
    Self {
      tabs: desktop.navigation.order.clone(),
      active_tab: desktop.navigation.active,
      unbound: desktop
        .navigation
        .unbound
        .iter()
        .map(|(id, original)| (*id, original.clone()))
        .collect(),
      session: desktop.vm.selected.clone(),
      show_sidebar: desktop.show_sidebar,
      show_workbench: desktop.show_workbench,
      theme: cx.theme().mode,
    }
  }

  /// Compare against the live desktop without allocating.
  fn matches(&self, desktop: &Desktop, cx: &App) -> bool {
    self.tabs == desktop.navigation.order
      && self.active_tab == desktop.navigation.active
      && self.session == desktop.vm.selected
      && self.show_sidebar == desktop.show_sidebar
      && self.show_workbench == desktop.show_workbench
      && self.theme == cx.theme().mode
      && self.unbound.len() == desktop.navigation.unbound.len()
      && self
        .unbound
        .iter()
        .all(|(id, original)| desktop.navigation.unbound.get(id) == Some(original))
  }
}

impl Desktop {
  /// Mark local state dirty and persist it now when persistence is ready.
  ///
  /// Entity observers call this for composer, workspace, layout, dock and
  /// workbench changes. A flush that returns early keeps the marker, so the
  /// next frame retries instead of dropping the change.
  pub(crate) fn queue_snapshot(&mut self, cx: &mut Context<Self>) {
    self.local_state_dirty = true;
    self.flush_snapshot(cx);
  }

  /// Frame entry point: one flag check plus one cheap key comparison, so an
  /// unchanged frame never serializes or clones the desktop state.
  pub(crate) fn flush_snapshot_if_dirty(&mut self, cx: &mut Context<Self>) {
    if !self.local_state_dirty && self.local_state_key.matches(self, cx) {
      return;
    }
    self.local_state_dirty = true;
    self.flush_snapshot(cx);
  }

  fn persistence_ready(&self) -> bool {
    self.persistence_sender.is_some()
      && self.persistence_scope.is_some()
      && self.persistence_loaded
      && !self.persistence_loading
  }

  fn flush_snapshot(&mut self, cx: &mut Context<Self>) {
    if !self.local_state_dirty || !self.persistence_ready() {
      return;
    }
    let (Some(scope), Some(sender)) = (
      self.persistence_scope.clone(),
      self.persistence_sender.clone(),
    ) else {
      return;
    };
    self.local_state_dirty = false;
    self.local_state_key = SnapshotKey::capture(self, cx);
    let state = DesktopState {
      tabs: self.persisted_tabs(cx),
      active_tab: self.navigation.active,
      show_sidebar: self.show_sidebar,
      show_workbench: self.show_workbench,
      dock: self.dock.read(cx).dump(cx),
      layout_sizes: self
        .main_layout
        .read(cx)
        .sizes()
        .iter()
        .map(|size| size.as_f32())
        .collect(),
      theme: cx.theme().mode,
      editor_drafts: self.tasks.read(cx).persisted_editor_drafts(cx),
    };
    if let Err(error) = state.validate() {
      self.vm.error = Some(format!(
        "Local desktop state was not saved because it cannot be safely restored: {error}"
      ));
      cx.notify();
      return;
    }
    let Ok(encoded) = serde_json::to_string(&state) else {
      return;
    };
    if self.last_enqueued.as_deref() == Some(encoded.as_str()) {
      return;
    }
    self.persistence_sequence = self.persistence_sequence.saturating_add(1);
    self.last_enqueued = Some(encoded);
    self.latest_state = Some(state.clone());
    if let Ok(mut flush) = self.flush_control.lock() {
      flush.sequence = self.persistence_sequence;
      flush.state = Some(state.clone());
    }
    if let Ok(mut latest) = self.latest_shared.lock() {
      *latest = Some((self.persistence_sequence, state));
    }
    let command = Command::Save {
      scope,
      generation: self.persistence_generation,
      sequence: self.persistence_sequence,
    };
    let _ = sender.try_send(command);
  }
}

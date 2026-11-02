use crate::{shell::Desktop, view_model::DesktopViewModel};
use gpui_kit::{
  component::input::{InputState, TextareaState},
  *,
};
use std::collections::BTreeMap;

pub(crate) struct Navigation {
  pub active: u64,
  pub order: Vec<u64>,
  /// Inactive tabs; each entry owns its own `DesktopViewModel`.
  pub inactive: BTreeMap<u64, ChatState>,
  pub unbound: BTreeMap<u64, String>,
  pub connecting: bool,
  pub pending_requests: usize,
  pub(super) next_id: u64,
}

impl Default for Navigation {
  fn default() -> Self {
    Self {
      active: 1,
      order: vec![1],
      inactive: BTreeMap::new(),
      unbound: BTreeMap::new(),
      connecting: false,
      pending_requests: 0,
      next_id: 2,
    }
  }
}

impl Navigation {
  pub fn allocate(&mut self) -> Option<u64> {
    if self.order.len() >= crate::local_state::MAX_TABS {
      return None;
    }
    let id = self.next_id;
    self.next_id = id.checked_add(1)?;
    self.order.push(id);
    Some(id)
  }
}

/// An inactive chat tab. The active tab's equivalent state lives on `Desktop`
/// (`Desktop.vm`, `composer`, `workspace`, `watch_task`).
pub(crate) struct ChatState {
  pub vm: DesktopViewModel,
  pub(super) composer: Entity<TextareaState>,
  pub(super) workspace: Entity<InputState>,
  _persistence_watch: [Subscription; 2],
  pub watch_task: Option<tokio::task::JoinHandle<()>>,
}

impl ChatState {
  pub fn new(
    workspace: String,
    connected: bool,
    window: &mut Window,
    cx: &mut Context<Desktop>,
  ) -> Self {
    let composer = cx.new(|cx| {
      TextareaState::new(window, cx)
        .submit_on_enter(true)
        .placeholder("描述你的任务，或补充要求…")
    });
    let workspace = cx.new(|cx| {
      InputState::new(window, cx)
        .default_value(workspace)
        .placeholder("Workspace path on daemon")
    });
    let persistence_watch = [
      cx.observe(&composer, |desktop, _, cx| desktop.queue_snapshot(cx)),
      cx.observe(&workspace, |desktop, _, cx| desktop.queue_snapshot(cx)),
    ];
    let vm = DesktopViewModel {
      status: if connected {
        "Connected · authenticated"
      } else {
        "Disconnected"
      }
      .into(),
      ..DesktopViewModel::default()
    };
    Self {
      vm,
      watch_task: None,
      composer,
      workspace,
      _persistence_watch: persistence_watch,
    }
  }

  /// The single exchange point between the active tab (`Desktop`) and this
  /// inactive tab. Nothing else may move per-tab state, and window-level state
  /// such as `Desktop.sessions` is never swapped.
  pub fn swap_with(&mut self, desktop: &mut Desktop) {
    std::mem::swap(&mut self.vm, &mut desktop.vm);
    std::mem::swap(&mut self.composer, &mut desktop.composer);
    std::mem::swap(&mut self.workspace, &mut desktop.workspace);
    std::mem::swap(&mut self.watch_task, &mut desktop.watch_task);
  }

  pub fn reset_connection_preserving_session(&mut self) {
    let vm = &mut self.vm;
    if let Some(state) = &mut vm.recovery {
      state.inspected = false;
    }
    vm.runs.clear();
    vm.active_run = None;
    vm.cursor = 0;
    vm.busy = false;
    vm.error = None;
    vm.status = "Connected · authenticated".into();
    vm.conversation.clear();
    vm.approvals.clear();
    if let Some(task) = self.watch_task.take() {
      task.abort();
    }
  }
}

impl Drop for ChatState {
  fn drop(&mut self) {
    if let Some(task) = self.watch_task.take() {
      task.abort();
    }
  }
}

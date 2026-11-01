use crate::{message::ChatMessage, shell::Desktop};
use agent_protocol::v1::{Approval, Run};
use gpui_kit::{
  component::input::{InputState, TextareaState},
  *,
};
use std::collections::BTreeMap;

pub(crate) struct Navigation {
  pub active: u64,
  pub order: Vec<u64>,
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
    if self.order.len() >= 64 {
      return None;
    }
    let id = self.next_id;
    self.next_id = id.checked_add(1)?;
    self.order.push(id);
    Some(id)
  }
}

pub(crate) struct ChatState {
  pub selected: Option<String>,
  pub runs: Vec<Run>,
  pub active_run: Option<String>,
  cursor: u64,
  pub busy: bool,
  error: Option<String>,
  status: String,
  conversation: Vec<ChatMessage>,
  pub approvals: Vec<Approval>,
  recovery: Option<crate::recovery::RecoveryState>,
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
    Self {
      selected: None,
      runs: Vec::new(),
      active_run: None,
      cursor: 0,
      busy: false,
      error: None,
      status: if connected {
        "Connected · authenticated"
      } else {
        "Disconnected"
      }
      .into(),
      conversation: Vec::new(),
      approvals: Vec::new(),
      recovery: None,
      watch_task: None,
      composer,
      workspace,
      _persistence_watch: persistence_watch,
    }
  }

  pub fn swap_with(&mut self, desktop: &mut Desktop) {
    std::mem::swap(&mut self.selected, &mut desktop.vm.selected);
    std::mem::swap(&mut self.runs, &mut desktop.vm.runs);
    std::mem::swap(&mut self.active_run, &mut desktop.vm.active_run);
    std::mem::swap(&mut self.cursor, &mut desktop.vm.cursor);
    std::mem::swap(&mut self.busy, &mut desktop.vm.busy);
    std::mem::swap(&mut self.error, &mut desktop.vm.error);
    std::mem::swap(&mut self.status, &mut desktop.vm.status);
    std::mem::swap(&mut self.conversation, &mut desktop.vm.conversation);
    std::mem::swap(&mut self.approvals, &mut desktop.vm.approvals);
    std::mem::swap(&mut self.recovery, &mut desktop.recovery);
    std::mem::swap(&mut self.composer, &mut desktop.composer);
    std::mem::swap(&mut self.workspace, &mut desktop.workspace);
    std::mem::swap(&mut self.watch_task, &mut desktop.watch_task);
  }

  pub fn reset_connection_preserving_session(&mut self) {
    if let Some(state) = &mut self.recovery {
      state.inspected = false;
    }
    self.runs.clear();
    self.active_run = None;
    self.cursor = 0;
    self.busy = false;
    self.error = None;
    self.status = "Connected · authenticated".into();
    self.conversation.clear();
    self.approvals.clear();
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

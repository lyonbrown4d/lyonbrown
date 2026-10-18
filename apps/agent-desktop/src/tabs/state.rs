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
  pub connecting: bool,
  pub pending_requests: usize,
  next_id: u64,
}

impl Default for Navigation {
  fn default() -> Self {
    Self {
      active: 1,
      order: vec![1],
      inactive: BTreeMap::new(),
      connecting: false,
      pending_requests: 0,
      next_id: 2,
    }
  }
}

impl Navigation {
  pub fn allocate(&mut self) -> u64 {
    let id = self.next_id;
    self.next_id += 1;
    self.order.push(id);
    id
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
  composer: Entity<TextareaState>,
  workspace: Entity<InputState>,
  pub watch_task: Option<tokio::task::JoinHandle<()>>,
}

impl ChatState {
  pub fn new(
    workspace: String,
    connected: bool,
    window: &mut Window,
    cx: &mut Context<Desktop>,
  ) -> Self {
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
      watch_task: None,
      composer: cx.new(|cx| {
        TextareaState::new(window, cx).placeholder("Ask Lyonbrown to work on your project…")
      }),
      workspace: cx.new(|cx| {
        InputState::new(window, cx)
          .default_value(workspace)
          .placeholder("Workspace path on daemon")
      }),
    }
  }

  pub fn swap_with(&mut self, desktop: &mut Desktop) {
    std::mem::swap(&mut self.selected, &mut desktop.selected);
    std::mem::swap(&mut self.runs, &mut desktop.runs);
    std::mem::swap(&mut self.active_run, &mut desktop.active_run);
    std::mem::swap(&mut self.cursor, &mut desktop.cursor);
    std::mem::swap(&mut self.busy, &mut desktop.busy);
    std::mem::swap(&mut self.error, &mut desktop.error);
    std::mem::swap(&mut self.status, &mut desktop.status);
    std::mem::swap(&mut self.conversation, &mut desktop.conversation);
    std::mem::swap(&mut self.approvals, &mut desktop.approvals);
    std::mem::swap(&mut self.composer, &mut desktop.composer);
    std::mem::swap(&mut self.workspace, &mut desktop.workspace);
    std::mem::swap(&mut self.watch_task, &mut desktop.watch_task);
  }

  pub fn reset_connection(&mut self) {
    self.selected = None;
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

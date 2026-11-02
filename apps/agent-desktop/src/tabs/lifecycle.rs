//! Chat-tab lifecycle: activation, creation, closing, and daemon binding. The
//! switch guard itself lives in `crate::block`.

use super::{TabSender, state::ChatState};
use crate::shell::Desktop;
use gpui_kit::*;

impl Desktop {
  pub(crate) fn scoped_sender(&self) -> TabSender {
    TabSender {
      id: self.navigation.active,
      sender: self.sender.clone(),
    }
  }

  pub(crate) fn any_active_work(&self) -> bool {
    self.navigation.connecting
      || self.navigation.pending_requests > 0
      || self.vm.busy
      || self.vm.active_run.is_some()
      || self
        .navigation
        .inactive
        .values()
        .any(|state| state.vm.busy || state.vm.active_run.is_some())
  }

  pub(crate) fn activate_chat_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
    if !self.can_change_chat(cx) || id == self.navigation.active {
      return;
    }
    let Some(mut state) = self.navigation.inactive.remove(&id) else {
      return;
    };
    let previous = self.navigation.active;
    state.swap_with(self);
    self.navigation.inactive.insert(previous, state);
    self.navigation.active = id;
    self.bind_active_chat(window, cx);
    if self.vm.selected.is_some() && self.vm.runs.is_empty() {
      self.refresh_restored_session(cx);
    }
    self.composer.read(cx).focus_handle(cx).focus(window, cx);
    cx.notify();
  }

  pub(crate) fn new_chat_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if !self.can_change_chat(cx) {
      return;
    }
    self.router.return_to_chat();
    if !self.add_blank_chat(window, cx) {
      return;
    }
    if !self.navigation.connecting {
      self.create_session(cx);
    }
  }

  pub(crate) fn open_session_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
    self.add_blank_chat(window, cx)
  }

  fn add_blank_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
    let workspace = self.workspace.read(cx).value().to_string();
    let state = ChatState::new(workspace, self.client.is_some(), window, cx);
    let Some(id) = self.navigation.allocate() else {
      self.vm.error = Some("Could not allocate another local chat tab.".into());
      cx.notify();
      return false;
    };
    self.navigation.inactive.insert(id, state);
    self.activate_chat_tab(id, window, cx);
    true
  }

  pub(crate) fn close_active_chat_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.close_chat_tab(self.navigation.active, window, cx);
  }

  pub(crate) fn close_chat_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
    let Some(index) = self
      .navigation
      .order
      .iter()
      .position(|candidate| *candidate == id)
    else {
      return;
    };
    if self.navigation.active == id {
      if !self.can_change_chat(cx) {
        return;
      }
      if self.navigation.order.len() == 1 && !self.add_blank_chat(window, cx) {
        return;
      }
      let next = self
        .navigation
        .order
        .iter()
        .copied()
        .filter(|candidate| *candidate != id)
        .nth(index.saturating_sub(1))
        .or_else(|| {
          self
            .navigation
            .order
            .iter()
            .copied()
            .find(|candidate| *candidate != id)
        });
      if let Some(next) = next {
        self.activate_chat_tab(next, window, cx);
      }
    }
    self.navigation.order.retain(|candidate| *candidate != id);
    self.navigation.unbound.remove(&id);
    // Dropping a tab stops only its local subscription, never its daemon run.
    self.navigation.inactive.remove(&id);
    cx.notify();
  }

  pub(crate) fn bind_active_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self
      .resources
      .bind(self.client.clone(), self.vm.selected.clone(), cx);
    self.tasks.update(cx, |tasks, cx| {
      tasks.bind(
        self.client.clone(),
        self.vm.selected.clone(),
        self.vm.runs.clone(),
        window,
        cx,
      )
    });
  }
}

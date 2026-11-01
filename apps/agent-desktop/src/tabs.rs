mod delivery;
mod state;
mod view;

use crate::{connection::Update, shell::Desktop};
use delivery::PendingWatch;
pub(super) use delivery::TabSender;
use gpui_kit::*;
use state::ChatState;
pub(super) use state::Navigation;

pub(super) const COMPUTER_WAIT_MESSAGE: &str =
  "Wait for the computer operation to finish before changing chats or the connection.";

pub(super) const SAVE_WAIT_MESSAGE: &str =
  "Wait for the file save to finish before switching chats.";
pub(super) const BROWSER_WAIT_MESSAGE: &str =
  "Wait for the browser operation to finish before changing chats or the connection.";

pub(super) enum ScopedUpdate {
  Tab {
    id: u64,
    update: Update,
  },
  Watch {
    id: u64,
    task: PendingWatch,
  },
  Connection(Update),
  RequestFinished,
  Persistence(crate::persistence::ResultMessage),
  RestoredRuns {
    scope: String,
    generation: u64,
    session_id: String,
    runs: Vec<agent_protocol::v1::Run>,
  },
}

/// A logical chat's presentation identity; sessions and runs remain daemon-owned.
#[derive(Clone, Debug)]
pub struct ChatTabSnapshot {
  pub id: u64,
  pub title: String,
  pub session_id: Option<String>,
  pub active_run: Option<String>,
  pub selected: bool,
}

impl Desktop {
  pub(super) fn scoped_sender(&self) -> TabSender {
    TabSender {
      id: self.navigation.active,
      sender: self.sender.clone(),
    }
  }

  pub(super) fn apply_scoped(
    &mut self,
    update: ScopedUpdate,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let (id, update) = match update {
      ScopedUpdate::RequestFinished => {
        self.navigation.pending_requests = self.navigation.pending_requests.saturating_sub(1);
        cx.notify();
        return;
      }
      ScopedUpdate::Persistence(result) => {
        self.apply_persistence(result, window, cx);
        return;
      }
      ScopedUpdate::RestoredRuns {
        scope,
        generation,
        session_id,
        runs,
      } => {
        if self.persistence_scope.as_deref() == Some(scope.as_str())
          && self.persistence_generation == generation
          && self.vm.selected.as_deref() == Some(session_id.as_str())
        {
          self.apply(Update::Runs(runs), window, cx, true);
        }
        return;
      }
      ScopedUpdate::Connection(update) => {
        self.navigation.connecting = false;
        self.tasks.update(cx, |tasks, cx| {
          tasks.connection_pending = false;
          tasks.computer.update(cx, |computer, cx| {
            computer.connection_pending = false;
            cx.notify();
          });
          cx.notify();
        });
        self.apply(update, window, cx, true);
        return;
      }
      ScopedUpdate::Watch { id, task } => {
        let Some(task) = task.into_inner() else {
          return;
        };
        let slot = if id == self.navigation.active {
          Some(&mut self.watch_task)
        } else {
          self
            .navigation
            .inactive
            .get_mut(&id)
            .map(|state| &mut state.watch_task)
        };
        if let Some(slot) = slot {
          if let Some(previous) = slot.replace(task) {
            previous.abort();
          }
        } else {
          task.abort();
        }
        return;
      }
      ScopedUpdate::Tab { id, update } => (id, update),
    };
    if matches!(update, Update::Session(..)) {
      self.navigation.unbound.remove(&id);
    }
    if id == self.navigation.active {
      self.apply(update, window, cx, true);
    } else if let Some(mut state) = self.navigation.inactive.remove(&id) {
      let visible = self.navigation.active;
      state.swap_with(self);
      self.navigation.active = id;
      self.apply(update, window, cx, false);
      state.swap_with(self);
      self.navigation.active = visible;
      self.navigation.inactive.insert(id, state);
      cx.notify();
    } else if let Update::Session(session, _) = update {
      // Creating a daemon session is durable even when its requesting tab was closed.
      self
        .vm
        .sessions
        .retain(|previous| previous.id != session.id);
      self.vm.sessions.insert(0, session);
      cx.notify();
    }
  }

  pub(super) fn any_active_work(&self) -> bool {
    self.navigation.connecting
      || self.navigation.pending_requests > 0
      || self.vm.busy
      || self.vm.active_run.is_some()
      || self
        .navigation
        .inactive
        .values()
        .any(|state| state.busy || state.active_run.is_some())
  }

  pub(super) fn can_change_chat(&mut self, cx: &mut Context<Self>) -> bool {
    if self.navigation.connecting || self.persistence_loading {
      return false;
    }
    if self.recovery.as_ref().is_some_and(|state| state.pending) {
      self.vm.error = Some("等待异常运行核对或恢复请求结束后再切换聊天。".into());
      cx.notify();
      return false;
    }
    if self.tasks.read(cx).computer.read(cx).pending {
      self.vm.error = Some(COMPUTER_WAIT_MESSAGE.into());
      cx.notify();
      return false;
    }
    if self.tasks.read(cx).pending_save {
      self.vm.error = Some(SAVE_WAIT_MESSAGE.into());
      cx.notify();
      return false;
    }
    if self.tasks.read(cx).pending_browser_operation {
      self.vm.error = Some(BROWSER_WAIT_MESSAGE.into());
      cx.notify();
      return false;
    }
    true
  }

  pub(super) fn activate_chat_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
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

  pub(super) fn new_chat_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if !self.can_change_chat(cx) {
      return;
    }
    self.show_settings = false;
    if !self.add_blank_chat(window, cx) {
      return;
    }
    if !self.navigation.connecting {
      self.create_session(cx);
    }
  }

  pub(super) fn open_session_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
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

  pub(super) fn close_active_chat_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.close_chat_tab(self.navigation.active, window, cx);
  }

  pub(super) fn close_chat_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
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

  pub(super) fn bind_active_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

  pub fn chat_tab_snapshots(&self, _cx: &App) -> Vec<ChatTabSnapshot> {
    self
      .navigation
      .order
      .iter()
      .filter_map(|id| {
        let (session, run) = if *id == self.navigation.active {
          (&self.vm.selected, &self.vm.active_run)
        } else {
          let state = self.navigation.inactive.get(id)?;
          (&state.selected, &state.active_run)
        };
        let title = if self.navigation.unbound.contains_key(id) {
          "Unbound restored draft".into()
        } else {
          session
            .as_ref()
            .and_then(|id| self.vm.sessions.iter().find(|session| &session.id == id))
            .map(|session| session.title.clone())
            .unwrap_or_else(|| "New chat".into())
        };
        Some(ChatTabSnapshot {
          id: *id,
          title,
          session_id: session.clone(),
          active_run: run.clone(),
          selected: *id == self.navigation.active,
        })
      })
      .collect()
  }

  pub(super) fn persisted_tabs(&self, cx: &App) -> Vec<crate::local_state::TabState> {
    self
      .navigation
      .order
      .iter()
      .filter_map(|id| {
        if *id == self.navigation.active {
          Some(crate::local_state::TabState {
            id: *id,
            session_id: self.vm.selected.clone(),
            unbound_from: self.navigation.unbound.get(id).cloned(),
            composer: self.composer.read(cx).value().to_string(),
            workspace: self.workspace.read(cx).value().to_string(),
          })
        } else {
          let state = self.navigation.inactive.get(id)?;
          Some(crate::local_state::TabState {
            id: *id,
            session_id: state.selected.clone(),
            unbound_from: self.navigation.unbound.get(id).cloned(),
            composer: state.composer.read(cx).value().to_string(),
            workspace: state.workspace.read(cx).value().to_string(),
          })
        }
      })
      .collect()
  }

  pub(super) fn restore_tabs(
    &mut self,
    tabs: &[crate::local_state::TabState],
    active: u64,
    sessions: &std::collections::HashSet<String>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if tabs.is_empty() {
      return;
    }
    let active_tab = tabs.iter().find(|tab| tab.id == active).unwrap_or(&tabs[0]);
    self.navigation.inactive.clear();
    self.navigation.unbound.clear();
    self.navigation.order.clear();
    self.navigation.active = active_tab.id;
    self.navigation.next_id = tabs
      .iter()
      .map(|tab| tab.id)
      .max()
      .unwrap_or(0)
      .saturating_add(1)
      .max(1);
    for tab in tabs {
      self.navigation.order.push(tab.id);
      let selected = tab.session_id.clone().filter(|id| sessions.contains(id));
      let unbound_from = selected
        .is_none()
        .then(|| tab.unbound_from.clone().or_else(|| tab.session_id.clone()))
        .flatten();
      if let Some(original) = unbound_from.clone() {
        self.navigation.unbound.insert(tab.id, original);
      }
      if tab.id == active_tab.id {
        self.workspace.update(cx, |workspace, cx| {
          workspace.set_value(tab.workspace.clone(), window, cx)
        });
        self.composer.update(cx, |composer, cx| {
          composer.set_value(tab.composer.clone(), window, cx)
        });
        self.vm.selected = selected;
        self.vm.runs.clear();
        self.vm.active_run = None;
        self.vm.cursor = 0;
        self.vm.approvals.clear();
        self.vm.conversation.clear();
      } else {
        let mut state = ChatState::new(tab.workspace.clone(), self.client.is_some(), window, cx);
        state.selected = selected;
        state.composer.update(cx, |composer, cx| {
          composer.set_value(tab.composer.clone(), window, cx)
        });
        self.navigation.inactive.insert(tab.id, state);
      }
    }
    self.bind_active_chat(window, cx);
  }
}

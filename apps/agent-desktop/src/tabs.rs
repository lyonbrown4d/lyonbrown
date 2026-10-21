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
  Tab { id: u64, update: Update },
  Watch { id: u64, task: PendingWatch },
  Connection(Update),
  RequestFinished,
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
        if matches!(update, Update::Connected(..)) {
          for state in self.navigation.inactive.values_mut() {
            state.reset_connection();
          }
        }
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
    self.composer.read(cx).focus_handle(cx).focus(window, cx);
    cx.notify();
  }

  pub(super) fn new_chat_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if !self.can_change_chat(cx) {
      return;
    }
    self.show_settings = false;
    self.add_blank_chat(window, cx);
    if !self.navigation.connecting {
      self.create_session(cx);
    }
  }

  pub(super) fn open_session_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.add_blank_chat(window, cx);
  }

  fn add_blank_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let workspace = self.workspace.read(cx).value().to_string();
    let state = ChatState::new(workspace, self.client.is_some(), window, cx);
    let id = self.navigation.allocate();
    self.navigation.inactive.insert(id, state);
    self.activate_chat_tab(id, window, cx);
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
      if self.navigation.order.len() == 1 {
        self.add_blank_chat(window, cx);
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
        let title = session
          .as_ref()
          .and_then(|id| self.vm.sessions.iter().find(|session| &session.id == id))
          .map(|session| session.title.clone())
          .unwrap_or_else(|| "New chat".into());
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
}

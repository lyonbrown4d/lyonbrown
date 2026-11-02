//! Chat tabs: the window-level update envelope and the single dispatcher that
//! routes a scoped update to the active or an inactive tab. Tab lifecycle lives
//! in `tabs::lifecycle`, tab snapshots and restoration in `tabs::snapshot`.

mod delivery;
mod lifecycle;
mod snapshot;
mod state;
mod view;

use crate::{connection::Update, shell::Desktop};
use delivery::PendingWatch;
pub(super) use delivery::TabSender;
use gpui_kit::*;
pub(super) use state::Navigation;

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
      self.sessions.retain(|previous| previous.id != session.id);
      self.sessions.insert(0, session);
      cx.notify();
    }
  }
}

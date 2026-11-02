use crate::{shell::Desktop, tabs::ScopedUpdate};
mod command;
mod flush;
mod layout;
mod snapshot;
mod worker;
pub(crate) use self::command::{Command, LatestSnapshot, ResultMessage};
pub(crate) use self::flush::{FlushControl, FlushState, quit_flush};
pub(crate) use self::snapshot::SnapshotKey;
use self::worker::{ActorConfig, start};
use gpui_kit::component::Theme;
use gpui_kit::*;
use std::collections::HashSet;

impl Desktop {
  pub(super) fn start_persistence(
    &mut self,
    endpoint: &str,
    node_id: &str,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let scope = match agent_client::local_state::identity("desktop", endpoint, node_id) {
      Ok(scope) => scope,
      Err(error) => {
        self.persistence_loading = false;
        self.vm.error = Some(format!("Could not identify local desktop state: {error}"));
        cx.notify();
        return;
      }
    };
    if self.persistence_scope.as_deref() == Some(scope.as_str()) {
      return;
    }
    let previous = if let (Some(sender), Some(previous_scope), Some(state)) = (
      &self.persistence_sender,
      &self.persistence_scope,
      &self.latest_state,
    ) {
      let sender = sender.clone();
      let (command, completion) = Command::stop(
        previous_scope.clone(),
        self.persistence_generation,
        self.persistence_sequence,
        Some(state.clone()),
      );
      self.runtime.spawn(async move {
        let _ = sender.send(command).await;
      });
      Some(completion)
    } else {
      None
    };
    self.persistence_generation = self.persistence_generation.saturating_add(1);
    if self.persistence_scope.is_some() {
      // The previous scope's final DTO was captured above. Start the next
      // authenticated identity with empty local inputs until its own load ends.
      self.restore_tabs(
        &[crate::local_state::TabState {
          id: 1,
          session_id: None,
          unbound_from: None,
          composer: String::new(),
          workspace: String::new(),
        }],
        1,
        &HashSet::new(),
        window,
        cx,
      );
      self.tasks.update(cx, |tasks, cx| {
        tasks.restore_editor_drafts(Vec::new(), &HashSet::new(), window, cx)
      });
    }
    self.persistence_scope = Some(scope.clone());
    self.persistence_loading = true;
    self.persistence_loaded = false;
    self.last_persisted = None;
    self.last_enqueued = None;
    self.latest_state = None;
    self.vm.error = None;
    let latest = std::sync::Arc::new(std::sync::Mutex::new(None));
    self.latest_shared = latest.clone();
    self.persistence_sender = Some(start(
      self.runtime.clone(),
      latest,
      ActorConfig {
        root: self.data_dir.clone(),
        endpoint: endpoint.to_owned(),
        node_id: node_id.to_owned(),
        scope,
        generation: self.persistence_generation,
      },
      previous,
      self.sender.clone(),
    ));
    if let Ok(mut flush) = self.flush_control.lock() {
      flush.sender = self.persistence_sender.clone();
      flush.scope = self.persistence_scope.clone();
      flush.generation = self.persistence_generation;
      flush.sequence = self.persistence_sequence;
      flush.state = None;
      flush.completion = None;
    }
    cx.notify();
  }

  pub(super) fn apply_persistence(
    &mut self,
    result: ResultMessage,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let (scope, generation) = match &result {
      ResultMessage::Loaded {
        scope, generation, ..
      }
      | ResultMessage::Failed {
        scope, generation, ..
      }
      | ResultMessage::Saved {
        scope, generation, ..
      }
      | ResultMessage::SaveFailed {
        scope, generation, ..
      } => (scope, generation),
    };
    let scope = scope.clone();
    let generation = *generation;
    if self.persistence_scope.as_deref() != Some(scope.as_str())
      || self.persistence_generation != generation
    {
      return;
    }
    self.persistence_loading = false;
    match result {
      ResultMessage::Loaded { state, .. } => {
        if let Some(state) = state {
          let state = *state;
          let sessions = self
            .sessions
            .iter()
            .map(|session| session.id.clone())
            .collect::<HashSet<_>>();
          self.restore_tabs(&state.tabs, state.active_tab, &sessions, window, cx);
          self.show_sidebar = state.show_sidebar;
          self.show_workbench = state.show_workbench;
          Theme::change(state.theme, Some(window), cx);
          if let Err(error) = self
            .dock
            .update(cx, |dock, cx| dock.load(state.dock, window, cx))
          {
            self.vm.error = Some(format!("Could not restore workbench layout: {error}"));
          }
          if !state.layout_sizes.is_empty() {
            self.restore_layout_after_frame(
              state.layout_sizes.clone(),
              scope.clone(),
              generation,
              window,
              cx,
            );
          }
          self.tasks.update(cx, |tasks, cx| {
            tasks.restore_editor_drafts(state.editor_drafts, &sessions, window, cx)
          });
          self.refresh_restored_session(cx);
        }
        self.persistence_loaded = true;
        self.queue_snapshot(cx);
      }
      ResultMessage::Failed { message, .. } => {
        self.persistence_sender = None;
        self.persistence_loaded = false;
        if let Ok(mut flush) = self.flush_control.lock() {
          flush.sender = None;
          flush.scope = None;
          flush.state = None;
        }
        self.vm.error = Some(format!(
          "Local desktop state could not be loaded; the file was preserved: {message}"
        ));
      }
      ResultMessage::Saved { sequence, .. } => {
        if sequence == self.persistence_sequence {
          self.last_persisted = self.last_enqueued.clone();
          if self
            .vm
            .error
            .as_deref()
            .is_some_and(|message| message.starts_with("Local desktop state could not be saved;"))
          {
            self.vm.error = None;
          }
        }
      }
      ResultMessage::SaveFailed {
        sequence, message, ..
      } => {
        if sequence == self.persistence_sequence {
          self.last_enqueued = None;
          self.vm.error = Some(format!(
            "Local desktop state could not be saved; your edits remain in memory: {message}"
          ));
        }
      }
    }
    cx.notify();
  }

  pub(super) fn refresh_restored_session(&mut self, _cx: &mut Context<Self>) {
    let (Some(client), Some(session_id), Some(scope)) = (
      self.client.clone(),
      self.vm.selected.clone(),
      self.persistence_scope.clone(),
    ) else {
      return;
    };
    let generation = self.persistence_generation;
    let sender = self.sender.clone();
    self.runtime.spawn(async move {
      if let Ok(runs) = client.list_runs(&session_id).await {
        let _ = sender
          .send(ScopedUpdate::RestoredRuns {
            scope,
            generation,
            session_id,
            runs,
          })
          .await;
      }
    });
  }
}

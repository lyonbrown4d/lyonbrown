use crate::{connection::Update, message::ChatMessage, operations::terminal, shell::Desktop};
use gpui_kit::*;

impl Desktop {
  pub(super) fn apply(
    &mut self,
    update: Update,
    window: &mut Window,
    cx: &mut Context<Self>,
    foreground: bool,
  ) {
    match update {
      Update::Recovery(id, report) => self.apply_recovery(id, *report, cx),
      Update::RecoveryError(id, error) => {
        if let Some(state) = self.recovery.as_mut().filter(|state| state.run_id == id) {
          state.pending = false;
          state.inspected = false;
          state.error = Some(error);
        }
      }
      Update::Connected {
        client,
        sessions,
        endpoint,
        node_id,
      } => {
        self.resources.invalidate(cx);
        self.tasks.update(cx, |tasks, cx| {
          tasks.reset_git(cx);
        });
        tracing::info!(
          operation = "client.connect",
          outcome = "authenticated",
          process_id = std::process::id(),
          session_count = sessions.len(),
          "Desktop client connected"
        );
        let scope = agent_client::local_state::identity("desktop", &endpoint, &node_id).ok();
        let same_identity = scope
          .as_deref()
          .is_some_and(|scope| self.persistence_scope.as_deref() == Some(scope));
        if same_identity {
          if let Some(state) = &mut self.recovery {
            state.inspected = false;
          }
          let client = *client;
          let known = sessions
            .iter()
            .map(|session| session.id.clone())
            .collect::<std::collections::HashSet<_>>();
          let previous = self.vm.selected.clone();
          let selected = previous.clone().filter(|id| known.contains(id));
          if selected.is_none()
            && let Some(previous) = previous
          {
            self
              .navigation
              .unbound
              .insert(self.navigation.active, previous);
          }
          self.tasks.update(cx, |state, cx| {
            state
              .computer
              .update(cx, |computer, cx| computer.reset_connection(window, cx));
            state.attachment = None;
            state.browser_sessions.clear();
            state.space = None;
            state.tab = None;
            state.bind(
              Some(client.clone()),
              selected.clone(),
              Vec::new(),
              window,
              cx,
            );
            state.client = Some(client.clone());
            state.refresh_after_reconnect(window, cx);
          });
          for state in self.navigation.inactive.values_mut() {
            state.reset_connection_preserving_session();
          }
          if let Some(task) = self.watch_task.take() {
            task.abort();
          }
          self.client = Some(client);
          self.vm.sessions = sessions;
          self.vm.selected = selected;
          self.vm.active_run = None;
          self.vm.cursor = 0;
          self.vm.error = None;
          self.vm.conversation.clear();
          self.vm.runs.clear();
          self.vm.approvals.clear();
          self.vm.status = "Connected · authenticated".into();
          self.vm.busy = false;
          self.refresh_restored_session(cx);
          self.start_persistence(&endpoint, &node_id, window, cx);
          self.bind_active_chat(window, cx);
          return;
        }
        self.tasks.update(cx, |state, cx| {
          state
            .computer
            .update(cx, |computer, cx| computer.reset_connection(window, cx));
          state.client = None;
          state.attachment = None;
          state.browser_sessions.clear();
          state.space = None;
          state.tab = None;
        });
        if let Some(task) = self.watch_task.take() {
          task.abort();
        }
        self.recovery = None;
        self.vm.active_run = None;
        self.vm.cursor = 0;
        self.vm.error = None;
        self.vm.selected = None;
        self.vm.conversation.clear();
        self.vm.runs.clear();
        self.vm.approvals.clear();
        self.client = Some(*client);
        self.vm.sessions = sessions;
        self.vm.status = "Connected · authenticated".into();
        self.vm.busy = false;
        self.start_persistence(&endpoint, &node_id, window, cx);
      }
      Update::Session(session, keep_busy) => {
        self.vm.selected = Some(session.id.clone());
        self
          .vm
          .sessions
          .retain(|previous| previous.id != session.id);
        self.vm.sessions.insert(0, session);
        self.vm.conversation.clear();
        self.vm.runs.clear();
        self.vm.approvals.clear();
        self.vm.error = None;
        self.vm.busy = keep_busy;
      }
      Update::Runs(mut runs) => {
        runs.sort_by_key(|run| run.created_at_ms);
        self.vm.conversation = runs
          .iter()
          .flat_map(|run| {
            let mut messages = vec![ChatMessage::new("You", run.prompt.clone(), cx)];
            if !run.result.is_empty() {
              messages.push(ChatMessage::new("Lyonbrown", run.result.clone(), cx));
            }
            if !run.error.is_empty() {
              messages.push(ChatMessage::new("Run error", run.error.clone(), cx));
            }
            messages
          })
          .collect();
        if let Some(run) = runs.iter().rev().find(|run| !terminal(&run.status)) {
          self.vm.active_run = Some(run.id.clone());
          self.vm.cursor = 0;
          self.inspect_run(cx);
        }
        self.vm.runs = runs;
        self.vm.busy = false;
      }
      Update::Started(run) => {
        if self.composer.read(cx).value().as_ref() == run.prompt {
          self
            .composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        }
        self
          .vm
          .conversation
          .push(ChatMessage::new("You", run.prompt.clone(), cx));
        self.vm.active_run = Some(run.id.clone());
        self.vm.cursor = 0;
        self.vm.status = format!("Run {} · {}", run.id, run.status);
        self.vm.runs.push(run);
        self.vm.busy = false;
      }
      Update::Event(event) => {
        if self.vm.active_run.as_deref() != Some(event.run_id.as_str())
          || event.cursor <= self.vm.cursor
        {
          return;
        }
        self.vm.cursor = event.cursor;
        self.vm.status = format!("{} · event {}", event.kind, event.cursor);
        let payload: serde_json::Value =
          serde_json::from_str(&event.payload_json).unwrap_or_default();
        if let Some(run) = self.vm.runs.iter_mut().find(|run| run.id == event.run_id) {
          if let Some(status) = payload["status"].as_str() {
            run.status = status.to_string();
          } else if event.kind == "approval_required" {
            run.status = "awaiting_approval".into();
          }
        }
        if event.kind == "text_delta" {
          if let Some(text) = payload["text"].as_str() {
            if let Some(message) = self
              .vm
              .conversation
              .last_mut()
              .filter(|message| message.role == "Lyonbrown")
            {
              message.append(text, cx);
            } else {
              self
                .vm
                .conversation
                .push(ChatMessage::new("Lyonbrown", text.to_string(), cx));
            }
          }
        } else if event.kind == "tool_started" || event.kind == "tool_finished" {
          self.vm.conversation.push(ChatMessage::new(
            "Tool",
            format!(
              "**{}** · {}\n\n```json\n{}\n```",
              payload["name"]
                .as_str()
                .or_else(|| payload["tool"].as_str())
                .unwrap_or("Tool"),
              event.kind,
              serde_json::to_string_pretty(&payload).unwrap_or_default()
            ),
            cx,
          ));
        }
      }
      Update::Run(run) => {
        if self.vm.selected.as_deref() != Some(run.session_id.as_str()) {
          return;
        }
        self.vm.status = format!("Run {} · {}", run.id, run.status);
        if !run.result.is_empty()
          && self
            .vm
            .conversation
            .last()
            .is_none_or(|message| message.role != "Lyonbrown")
        {
          self
            .vm
            .conversation
            .push(ChatMessage::new("Lyonbrown", run.result.clone(), cx));
        }
        if !run.error.is_empty() {
          self.vm.error = Some(run.error.clone());
        }
        if terminal(&run.status) {
          self.vm.active_run = None;
        }
        if let Some(previous) = self
          .vm
          .runs
          .iter_mut()
          .find(|previous| previous.id == run.id)
        {
          *previous = run;
        }
      }
      Update::Error(error) => {
        // Error text may originate in a tool/provider; persist only safe context.
        tracing::warn!(
          operation = "client.connect_or_request",
          outcome = "failed",
          process_id = std::process::id(),
          session_id = self.vm.selected.as_deref(),
          run_id = self.vm.active_run.as_deref(),
          client_present = self.client.is_some(),
          error_category = "connection_or_request_failed",
          "Desktop operation failed; inspect daemon state before retrying"
        );
        self.vm.error = Some(error);
        self.vm.busy = false;
        self.vm.status = if self.vm.active_run.is_some() {
          "Run outcome unknown · inspect daemon"
        } else {
          "Connection or request failed"
        }
        .into();
      }
      Update::Approvals(approvals) => {
        self.vm.approvals = approvals
          .into_iter()
          .filter(|approval| {
            approval.status.eq_ignore_ascii_case("pending")
              && self.vm.active_run.as_deref() == Some(approval.run_id.as_str())
          })
          .collect();
      }
    }
    if foreground {
      self.bind_active_chat(window, cx);
    }
    cx.notify();
  }
}

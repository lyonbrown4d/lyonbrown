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
      Update::Connected(client, sessions) => {
        self.tasks.update(cx, |state, _| {
          state.client = None;
          state.attachment = None;
          state.browser_sessions.clear();
          state.space = None;
          state.tab = None;
        });
        if let Some(task) = self.watch_task.take() {
          task.abort();
        }
        self.active_run = None;
        self.cursor = 0;
        self.error = None;
        self.selected = None;
        self.conversation.clear();
        self.runs.clear();
        self.approvals.clear();
        self.client = Some(*client);
        self.sessions = sessions;
        self.status = "Connected · authenticated".into();
        self.busy = false;
      }
      Update::Session(session, keep_busy) => {
        self.selected = Some(session.id.clone());
        self.sessions.retain(|previous| previous.id != session.id);
        self.sessions.insert(0, session);
        self.conversation.clear();
        self.runs.clear();
        self.approvals.clear();
        self.error = None;
        self.busy = keep_busy;
      }
      Update::Runs(mut runs) => {
        runs.sort_by_key(|run| run.created_at_ms);
        self.conversation = runs
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
          self.active_run = Some(run.id.clone());
          self.cursor = 0;
          self.inspect_run(cx);
        }
        self.runs = runs;
        self.busy = false;
      }
      Update::Started(run) => {
        if self.composer.read(cx).value().as_ref() == run.prompt {
          self
            .composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        }
        self
          .conversation
          .push(ChatMessage::new("You", run.prompt.clone(), cx));
        self.active_run = Some(run.id.clone());
        self.cursor = 0;
        self.status = format!("Run {} · {}", run.id, run.status);
        self.runs.push(run);
        self.busy = false;
      }
      Update::Event(event) => {
        if self.active_run.as_deref() != Some(event.run_id.as_str()) || event.cursor <= self.cursor
        {
          return;
        }
        self.cursor = event.cursor;
        self.status = format!("{} · event {}", event.kind, event.cursor);
        let payload: serde_json::Value =
          serde_json::from_str(&event.payload_json).unwrap_or_default();
        if let Some(run) = self.runs.iter_mut().find(|run| run.id == event.run_id) {
          if let Some(status) = payload["status"].as_str() {
            run.status = status.to_string();
          } else if event.kind == "approval_required" {
            run.status = "awaiting_approval".into();
          }
        }
        if event.kind == "text_delta" {
          if let Some(text) = payload["text"].as_str() {
            if let Some(message) = self
              .conversation
              .last_mut()
              .filter(|message| message.role == "Lyonbrown")
            {
              message.append(text, cx);
            } else {
              self
                .conversation
                .push(ChatMessage::new("Lyonbrown", text.to_string(), cx));
            }
          }
        } else if event.kind == "tool_started" || event.kind == "tool_finished" {
          self.conversation.push(ChatMessage::new(
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
        if self.selected.as_deref() != Some(run.session_id.as_str()) {
          return;
        }
        self.status = format!("Run {} · {}", run.id, run.status);
        if !run.result.is_empty()
          && self
            .conversation
            .last()
            .is_none_or(|message| message.role != "Lyonbrown")
        {
          self
            .conversation
            .push(ChatMessage::new("Lyonbrown", run.result.clone(), cx));
        }
        if !run.error.is_empty() {
          self.error = Some(run.error.clone());
        }
        if terminal(&run.status) {
          self.active_run = None;
        }
        if let Some(previous) = self.runs.iter_mut().find(|previous| previous.id == run.id) {
          *previous = run;
        }
      }
      Update::Error(error) => {
        self.error = Some(error);
        self.busy = false;
        self.status = if self.active_run.is_some() {
          "Run outcome unknown · inspect daemon"
        } else {
          "Connection or request failed"
        }
        .into();
      }
      Update::Approvals(approvals) => {
        self.approvals = approvals
          .into_iter()
          .filter(|approval| {
            approval.status.eq_ignore_ascii_case("pending")
              && self.active_run.as_deref() == Some(approval.run_id.as_str())
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

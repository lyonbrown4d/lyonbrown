use crate::{
  backend::Update,
  model::{App, Approval, Connection, Message, Session},
};

pub fn apply(app: &mut App, update: Update) {
  match update {
    Update::Computer { status, detail } => {
      app.computer = status;
      app.computer_detail = detail;
      app.computer_scroll = 0;
    }
    Update::Connected(sessions) => {
      let selected = app.selected_session().map(|session| session.id.clone());
      app.connection = Connection::Connected;
      app.streaming = None;
      app.sessions = sessions
        .into_iter()
        .map(|session| Session {
          id: session.id,
          title: session.title,
        })
        .collect();
      app.selected = selected
        .and_then(|id| app.sessions.iter().position(|session| session.id == id))
        .unwrap_or(0);
    }
    Update::Disconnected(reason) => app.connection = Connection::Disconnected(reason),
    Update::Selected(session) => {
      app.sessions.push(Session {
        id: session.id,
        title: session.title,
      });
      app.selected = app.sessions.len().saturating_sub(1);
      app.messages.clear();
      app.tasks.clear();
      app.approvals.clear();
      app.active_run = None;
      app.streaming = None;
    }
    Update::Runs(mut runs) => {
      let Some(session) = app.selected_session() else {
        return;
      };
      runs.retain(|run| run.session_id == session.id);
      runs.sort_by_key(|run| run.created_at_ms);
      app.tasks.clear();
      app.messages.clear();
      app.active_run = None;
      for run in runs {
        if !run.result.is_empty() && app.streaming.as_ref().is_some_and(|(id, _)| id == &run.id) {
          app.streaming = None;
        }
        app.tasks.push(format!("{}\n{}", run.id, run.status));
        app.messages.push(Message {
          role: "You".into(),
          content: run.prompt,
        });
        if !run.result.is_empty() {
          app.messages.push(Message {
            role: "Agent".into(),
            content: run.result,
          });
        }
        if !run.error.is_empty() {
          app.messages.push(Message {
            role: "Error".into(),
            content: run.error,
          });
        }
        if matches!(
          run.status.as_str(),
          "accepted" | "running" | "awaiting_approval"
        ) {
          app.active_run = Some(run.id);
        }
      }
      if app
        .streaming
        .as_ref()
        .is_some_and(|(id, _)| app.active_run.as_ref() != Some(id))
      {
        app.streaming = None;
      }
    }
    Update::Event(event) => {
      app.notice = format!("Task event · {} · cursor {}", event.kind, event.cursor);
      if event.kind == "text_delta"
        && app.active_run.as_deref() == Some(event.run_id.as_str())
        && let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
        && let Some(text) = payload.get("text").and_then(|value| value.as_str())
      {
        let (_, content) = app
          .streaming
          .get_or_insert_with(|| (event.run_id, String::new()));
        content.push_str(text);
      }
    }
    Update::Approvals { session, approvals } => {
      if app
        .selected_session()
        .is_some_and(|selected| selected.id == session)
      {
        app.approvals = approvals
          .into_iter()
          .map(|approval| Approval {
            id: approval.id,
            description: format!("{}\n{}", approval.tool, approval.arguments_json),
          })
          .collect();
      }
    }

    Update::ApprovalDecided(id) => app.approvals.retain(|approval| approval.id != id),
    Update::Notice(notice) => {
      app.submitting = false;
      app.notice = notice;
    }
    Update::Submitted(prompt) => {
      app.submitting = false;
      if app.input.text() == prompt {
        app.input.clear();
      }
    }
  }
}

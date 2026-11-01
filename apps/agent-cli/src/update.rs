use crate::{
  backend::Update,
  model::{App, Approval, Connection, Message, Session},
};

pub fn apply(app: &mut App, update: Update) {
  match update {
    Update::RecoveryResult {
      session,
      generation,
      source,
      value,
    } => {
      crate::recovery_update::apply(app, session, generation, source, value);
    }
    Update::Computer { status, detail } => {
      app.computer = status;
      app.computer_detail = detail;
      app.computer_scroll = 0;
    }
    Update::Connected(sessions) => {
      app.generation = app.generation.saturating_add(1);
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
    Update::LocalState {
      node_id,
      state,
      warning,
      preserve,
    } => {
      app.state_loaded = true;
      app.persistence_node_id = node_id;
      app.persistence_warning = warning.unwrap_or_default();
      if preserve {
        return;
      }
      if let Some(state) = state {
        app.generation = app.generation.saturating_add(1);
        app.drafts = state.drafts;
        app.unbound_draft = state.unbound_draft;
        if let Some(index) = state
          .selected_session
          .and_then(|id| app.sessions.iter().position(|session| session.id == id))
        {
          app.selected = index;
        }
        app.panel = state.panel.into();
        app.scroll = state.transcript_scroll;
        app.computer_scroll = state.context_scroll;
        app.restore_current_draft();
      } else {
        app.drafts.clear();
        app.unbound_draft.clear();
        app.restore_current_draft();
      }
    }
    Update::Disconnected(reason) => app.connection = Connection::Disconnected(reason),
    Update::Selected(session) => {
      app.generation = app.generation.saturating_add(1);
      app.save_current_draft();
      app.sessions.push(Session {
        id: session.id,
        title: session.title,
      });
      app.selected = app.sessions.len().saturating_sub(1);
      app.messages.clear();
      app.tasks.clear();
      app.task_rows.clear();
      app.task_cursor.clear();
      app.task_selected = 0;
      app.task_detail.clear();
      app.approvals.clear();
      app.active_run = None;
      app.streaming = None;
      app.restore_current_draft();
    }
    Update::Runs {
      session: response_session,
      generation,
      mut runs,
    } => {
      if generation != app.generation {
        return;
      }
      let Some(session) = app.selected_session() else {
        return;
      };
      if session.id != response_session {
        return;
      }
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
        if run.status == "uncertain" {
          app.messages.push(Message {
            role: "Recovery".into(),
            content: format!(
              "运行结果未知，原请求不会自动重放。先用 /run inspect {} 查看记录，再核对实际副作用。",
              run.id
            ),
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
    Update::Event { generation, event } => {
      if generation != app.generation {
        return;
      }
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
    Update::Approvals {
      session,
      generation,
      approvals,
    } => {
      if generation == app.generation
        && app
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
    Update::Submitted {
      session,
      prompt,
      generation,
    } => {
      app.submitting = false;
      if app
        .drafts
        .get(&session)
        .is_some_and(|draft| draft == &prompt)
      {
        app.drafts.remove(&session);
      }
      if app.recovered_draft_from.as_deref() == Some(session.as_str())
        && app.unbound_draft == prompt
      {
        app.unbound_draft.clear();
        app.recovered_draft_from = None;
      }
      if generation == app.generation
        && app
          .selected_session()
          .is_some_and(|selected| selected.id == session)
        && app.input.text() == prompt
      {
        app.input.clear();
        app.save_current_draft();
      }
    }
    Update::TaskResult {
      session,
      generation,
      source,
      append,
      value,
    } => {
      if generation != app.generation {
        return;
      }
      if session.as_deref().is_some_and(|id| {
        app
          .selected_session()
          .is_none_or(|selected| selected.id != id)
      }) {
        return;
      }
      if let Some(tasks) = value.get("tasks").and_then(serde_json::Value::as_array) {
        if append {
          let mut known = app
            .task_rows
            .iter()
            .filter_map(|task| {
              task
                .pointer("/envelope/task_id")
                .and_then(serde_json::Value::as_str)
            })
            .map(str::to_owned)
            .collect::<std::collections::HashSet<_>>();
          for task in tasks {
            if let Some(id) = task
              .pointer("/envelope/task_id")
              .and_then(serde_json::Value::as_str)
              && known.insert(id.to_owned())
            {
              app.task_rows.push(task.clone());
            }
          }
        } else {
          app.task_rows = tasks.clone();
          app.task_selected = 0;
        }
        app.task_cursor = value
          .get("next_task_id")
          .and_then(serde_json::Value::as_str)
          .unwrap_or_default()
          .to_owned();
        app.task_detail.clear();
      } else {
        app.task_detail =
          serde_json::to_string_pretty(&value).unwrap_or_else(|_| "Task result unavailable".into());
      }
      if let Some(source) = source
        && app.input.text() == source
        && app
          .selected_session()
          .is_some_and(|selected| session.as_deref().is_none_or(|id| selected.id == id))
      {
        app.input.clear();
        app.save_current_draft();
      }
    }
  }
}

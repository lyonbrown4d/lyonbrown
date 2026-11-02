use crate::{
  backend::Update,
  model::{App, Approval, Connection, Message, Session},
};

pub fn apply(app: &mut App, update: Update) {
  match update {
    Update::Inputs {
      generation,
      run,
      inputs,
    } => {
      if generation != app.generation {
        return;
      }
      let _ = run;
      for receipt in inputs {
        if let Some(existing) = app
          .input_receipts
          .iter_mut()
          .find(|value| value.entry_id == receipt.entry_id)
        {
          if !crate::run_input_ui::terminal_input_state(existing.state)
            || crate::run_input_ui::terminal_input_state(receipt.state)
          {
            *existing = receipt;
          }
        } else {
          app.input_receipts.push(receipt);
        }
      }
      app.notice = "输入状态已由 agentd 确认；F7 查看待处理、采纳和撤销记录。".into();
    }

    Update::RecoveryResult {
      session,
      generation,
      source,
      value,
    } => {
      crate::recovery_update::apply(app, session, generation, source, value);
    }
    Update::ResourceDetail {
      session,
      generation,
      navigation_revision,
      source,
      title,
      detail,
    } => {
      if generation != app.generation
        || session.as_deref() != app.selected_session().map(|value| value.id.as_str())
      {
        return;
      }
      // Command output belongs in the transcript; a side page would hide the conversation.
      // The composer was cleared at submission, so a result never touches newer typing.
      let _ = (navigation_revision, source);
      app.push_local(format!("{title}\n{detail}"));
    }
    Update::ModelCatalog {
      session,
      generation,
      models,
    } => {
      if generation != app.generation
        || session.as_deref() != app.selected_session().map(|value| value.id.as_str())
      {
        return;
      }
      let picker = crate::picker::Picker::models().with_models(&models);
      if picker.is_empty() {
        app.push_local("Models\n\nagentd 没有返回可用的模型。");
      } else {
        app.open_picker(picker);
      }
    }
    Update::ProviderProfile {
      generation,
      dialog,
      result,
    } => {
      crate::profile_dialog::apply(app, generation, &dialog, result);
    }
    Update::ProviderCredential {
      generation,
      dialog,
      result,
    } => {
      if generation != app.generation {
        return;
      }
      let Some(current) = app
        .provider_dialog
        .as_mut()
        .filter(|current| current.id == dialog)
      else {
        return;
      };
      current.busy = false;
      current.confirm_remove = false;
      match result {
        Ok(provider) if provider.id == current.provider.id => {
          if let Some(cached) = app
            .provider_catalog
            .iter_mut()
            .find(|cached| cached.id == provider.id)
          {
            *cached = provider.clone();
          }
          current.provider = provider;
          current.revision = current.revision.saturating_add(1);
          current.credential_present = false;
          current.message = "agentd 已确认；密钥输入已清空。可关闭后使用 /model 选择模型。".into();
        }
        Ok(_) => current.message = "响应身份不匹配，请关闭后刷新核对。".into(),
        Err(message) => current.message = message,
      }
    }
    Update::ProviderCatalog {
      generation,
      providers,
      profiles,
    } => {
      if generation != app.generation {
        return;
      }
      let candidates = crate::resource_command::candidate_environment_names();
      let picker =
        crate::picker::Picker::providers(&providers, &candidates).with_profiles(&profiles);
      app.provider_catalog = providers;
      app.provider_profiles = profiles;
      if picker.is_empty() {
        app.push_local("Providers\n\nagentd 没有返回提供方。");
      } else {
        app.open_picker(picker);
      }
    }
    Update::SkillsCatalog {
      session,
      generation,
      scope,
      skills,
    } => {
      if generation != app.generation
        || session.as_deref() != app.selected_session().map(|value| value.id.as_str())
      {
        return;
      }
      let picker = crate::picker::Picker::skills(&skills);
      if picker.is_empty() {
        app.push_local(format!("Skills · {scope}\n\n没有已安装的技能。"));
      } else {
        app.open_picker(picker);
      }
    }
    Update::WorkspaceDiffText {
      session,
      generation,
      path,
      text,
    } => {
      if generation != app.generation
        || Some(session.as_str()) != app.selected_session().map(|value| value.id.as_str())
      {
        return;
      }
      let document = crate::diff::document::parse(&text);
      if document.is_empty() {
        app.push_local(if path.is_empty() {
          "Git diff\n\n工作区没有未提交的改动。".to_owned()
        } else {
          format!("Git diff · {path}\n\n该文件没有改动。")
        });
      } else {
        app.open_diff(path, document);
      }
    }
    Update::Computer { status, detail } => {
      app.computer = status;
      app.computer_detail = detail;
      app
        .router
        .reset_scroll(crate::model::ContextPanel::Computer);
    }
    Update::Connected(sessions) => {
      app.router.rebind();
      app.generation = app.generation.saturating_add(1);
      let selected = app.selected_session().map(|session| session.id.clone());
      app.connection = Connection::Connected;
      app.activity = Default::default();
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
      app.sessions_cursor = app.selected;
    }
    Update::LocalState {
      node_id,
      state,
      warning,
      preserve,
    } => {
      app.state_loaded = true;
      app
        .interaction
        .bind_connection(node_id.as_deref().unwrap_or_default());
      app.persistence_node_id = node_id;
      app.persistence_warning = warning.unwrap_or_default();
      if preserve {
        return;
      }
      if let Some(state) = state {
        app.generation = app.generation.saturating_add(1);
        app.interaction.import_history(state.interaction);
        app.input_attempt = state.input_attempt;
        app.drafts = state.drafts;
        app.unbound_draft = state.unbound_draft;
        if let Some(index) = state
          .selected_session
          .and_then(|id| app.sessions.iter().position(|session| session.id == id))
        {
          app.selected = index;
        }
        app.router.restore(state.panel.into());
        app.scroll = state.transcript_scroll;
        app.router.set_scroll(state.context_scroll);
        app.restore_current_draft();
      } else {
        app.drafts.clear();
        app.unbound_draft.clear();
        app.restore_current_draft();
      }
    }
    Update::Disconnected(reason) => {
      app.provider_dialog = None;
      app.profile_dialog = None;
      app.connection = Connection::Disconnected(reason);
    }
    Update::Selected(session) => {
      app.router.rebind();
      app.generation = app.generation.saturating_add(1);
      app.save_current_draft();
      app.sessions.push(Session {
        id: session.id,
        title: session.title,
      });
      app.selected = app.sessions.len().saturating_sub(1);
      app.sessions_cursor = app.selected;
      app.activity = Default::default();
      app.workflow = Default::default();
      app.input_receipts.clear();
      app.input_selected = 0;
      app.transcript_anchor = None;
      app.reading = Default::default();
      app.messages.clear();
      app.message_keys.clear();
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
      app.message_keys.clear();
      app.active_run = None;
      for run in runs {
        if !run.result.is_empty() && app.streaming.as_ref().is_some_and(|(id, _)| id == &run.id) {
          app.streaming = None;
        }
        app.tasks.push(format!("{}\n{}", run.id, run.status));
        app.message_keys.push(format!("{}:prompt", run.id));
        app.messages.push(Message {
          role: "You".into(),
          content: run.prompt,
        });
        if !run.result.is_empty() {
          app.message_keys.push(format!("{}:result", run.id));
          app.messages.push(Message {
            role: "Agent".into(),
            content: run.result,
          });
        }
        if !run.error.is_empty() {
          app.message_keys.push(format!("{}:error", run.id));
          app.messages.push(Message {
            role: "Error".into(),
            content: run.error,
          });
        }
        if run.status == "uncertain" {
          app.message_keys.push(format!("{}:recovery", run.id));
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
      if !app.activity.apply(&event) {
        return;
      }
      crate::run_input_ui::apply_event(app, &event);
      app.notice = app.activity.summary();
      if app.active_run.as_deref() == Some(event.run_id.as_str())
        && let Ok(agent_core::events::DecodedEvent::Known(
          agent_core::events::AgentEvent::TextDelta { text },
        )) = agent_core::events::decode(&event.kind, &event.payload_json)
      {
        let (_, content) = app
          .streaming
          .get_or_insert_with(|| (event.run_id, String::new()));
        content.push_str(&text);
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
        app.workflow.approvals.replace(approvals.clone());
        app.approvals = approvals
          .into_iter()
          .map(|approval| Approval {
            id: approval.id,
            description: format!("{}\n{}", approval.tool, approval.arguments_json),
          })
          .collect();
      }
    }

    Update::ApprovalDecided(id) => {
      app.approvals.retain(|approval| approval.id != id);
      let remaining = app
        .workflow
        .approvals
        .rows()
        .iter()
        .filter(|approval| approval.id != id)
        .cloned()
        .collect();
      app.workflow.approvals.replace(remaining);
    }
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
      app.interaction.record_acknowledged(&session, &prompt);
      if app
        .input_attempt
        .as_ref()
        .is_some_and(|attempt| attempt.session == session && attempt.prompt == prompt)
      {
        app.input_attempt = None;
      }
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
        app.workflow.tasks.replace(app.task_rows.clone());
        if let Some(id) = app.workflow.tasks.selected_id() {
          app.task_selected = app
            .task_rows
            .iter()
            .position(|row| {
              row
                .pointer("/envelope/task_id")
                .and_then(serde_json::Value::as_str)
                == Some(id)
            })
            .unwrap_or(0);
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

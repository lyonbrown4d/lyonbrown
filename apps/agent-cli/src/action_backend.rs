use agent_client::AgentClient;
use tokio::sync::mpsc;

use crate::{
  backend::{Action, Update},
  session_backend::{refresh_session, watch},
};

/// Execute one daemon action. Durable task calls are one-shot; failure returns without replay.
pub(crate) async fn execute(
  client: &AgentClient,
  action: Action,
  selected: &mut Option<String>,
  selected_generation: &mut u64,
  stream: &mut Option<tokio::task::JoinHandle<()>>,
  updates: &mpsc::Sender<Update>,
) -> anyhow::Result<()> {
  match action {
    Action::Media(request) => {
      crate::media::execute(client, request, stream, updates).await?;
    }
    Action::CancelProviderProbe { .. } => {}
    Action::ProviderProfile {
      generation,
      dialog,
      operation,
    } => {
      let result = crate::profile_dialog::execute(client, operation).await;
      let _ = updates
        .send(Update::ProviderProfile {
          generation,
          dialog,
          result,
        })
        .await;
    }
    Action::ProviderCredential {
      provider,
      change,
      generation,
      dialog,
    } => {
      crate::provider_dialog::execute(client, provider, change, generation, dialog, updates).await;
    }
    Action::SubmitInput {
      attempt,
      generation,
    } => {
      anyhow::ensure!(
        client
          .node()
          .await?
          .capabilities
          .iter()
          .any(|capability| capability == "runs.input"),
        "daemon 不支持运行中输入；请升级 agentd。草稿保留，不会改为 StartRun"
      );
      let status = client.runtime_status(&attempt.session).await?;
      let epoch = status
        .session
        .ok_or_else(|| anyhow::anyhow!("session unavailable"))?
        .owner_epoch;
      let request_id = attempt.request_id.clone();
      let receipt = client
        .submit_run_input(agent_protocol::v1::RunInputRequest {
          run_id: attempt.run.clone(),
          owner_epoch: epoch,
          request_id: request_id.clone(),
          kind: match attempt.kind {
            agent_core::events::QueueKind::Steer => agent_protocol::v1::RunInputKind::Steer,
            agent_core::events::QueueKind::FollowUp => agent_protocol::v1::RunInputKind::FollowUp,
          } as i32,
          prompt: attempt.prompt.clone(),
        })
        .await
        .map_err(|_| {
          anyhow::anyhow!(
            "输入未确认；保留草稿与 request {request_id}，先在 F7 核对，同一草稿重试复用身份。"
          )
        })?;
      let _ = updates
        .send(Update::Submitted {
          session: attempt.session,
          prompt: attempt.prompt,
          generation,
        })
        .await;
      let _ = updates
        .send(Update::Inputs {
          generation,
          run: attempt.run,
          inputs: vec![receipt],
        })
        .await;
    }
    Action::ListInputs { run, generation } => {
      let snapshot = client.run_input_snapshot(&run).await?;
      let inputs = snapshot.inputs;
      let _ = updates
        .send(Update::Inputs {
          generation,
          run,
          inputs,
        })
        .await;
      if snapshot.history_truncated {
        let _ = updates
          .send(Update::Notice(
            "输入面板保留所有待处理项和最近 100 条终态记录；完整记录仍在 daemon 事件中。".into(),
          ))
          .await;
      }
    }
    Action::CancelInput {
      run,
      entry,
      generation,
    } => {
      let run_record = client.get_run(&run).await?;
      let epoch = client
        .runtime_status(&run_record.session_id)
        .await?
        .session
        .ok_or_else(|| anyhow::anyhow!("session unavailable"))?
        .owner_epoch;
      let receipt = client.cancel_run_input(&run, epoch, &entry).await?;
      let _ = updates
        .send(Update::Inputs {
          generation,
          run,
          inputs: vec![receipt],
        })
        .await;
    }
    Action::TaskOperation {
      task_id,
      operation,
      generation,
      session,
    } => {
      use crate::workflow::TaskOperation;
      let task = match operation {
        TaskOperation::Inspect => client.task(&task_id).await?,
        TaskOperation::Reconcile => client.reconcile_task_original(&task_id).await?,
        TaskOperation::Cancel => client.cancel_task_original(&task_id).await?,
        TaskOperation::Accept => client.accept_task_result_original(&task_id, true).await?,
        TaskOperation::Reject => client.accept_task_result_original(&task_id, false).await?,
      };
      let _ = updates
        .send(Update::TaskResult {
          session,
          generation,
          source: None,
          append: false,
          value: crate::task_management::task_json(&task),
        })
        .await;
    }

    Action::Connect | Action::Computer { .. } | Action::Shutdown(_) => {}
    Action::NewSession => {
      let workspace = std::env::current_dir()?.to_string_lossy().into_owned();
      let session = client
        .create_session("New conversation", &workspace)
        .await?;
      *selected = Some(session.id.clone());
      let _ = updates.send(Update::Selected(session)).await;
      let _ = updates
        .send(Update::Connected(client.list_sessions().await?))
        .await;
    }
    Action::Select {
      session: id,
      generation,
    } => {
      *selected = Some(id.clone());
      *selected_generation = generation;
      if let Some(task) = stream.take() {
        task.abort();
      }
      let active = refresh_session(client, &id, generation, updates).await?;
      if client
        .node()
        .await?
        .capabilities
        .iter()
        .any(|capability| capability == "runs.input")
        && let Some(run) = active.as_ref()
      {
        let inputs = client.list_run_inputs(run).await?;
        let _ = updates
          .send(Update::Inputs {
            generation,
            run: run.clone(),
            inputs,
          })
          .await;
      }
      *stream = Some(crate::session_backend::watch_session(
        client.clone(),
        id,
        active,
        generation,
        updates.clone(),
      ));
    }
    Action::Submit {
      session,
      prompt,
      generation,
    } => {
      let epoch = client
        .runtime_status(&session)
        .await?
        .session
        .ok_or_else(|| anyhow::anyhow!("session unavailable"))?
        .owner_epoch;
      let run = client
        .start_run_owned(&session, epoch, &prompt, &uuid::Uuid::now_v7().to_string())
        .await?;
      let _ = updates
        .send(Update::Submitted {
          session: session.clone(),
          prompt,
          generation,
        })
        .await;
      let _ = updates
        .send(Update::Notice(format!("Submitted task {}", run.id)))
        .await;
      refresh_session(client, &session, generation, updates).await?;
      if let Some(task) = stream.take() {
        task.abort();
      }
      *stream = Some(watch(client, &run.id, generation, updates).await?);
    }
    Action::Cancel(id) => {
      let run = client.cancel_run(&id).await?;
      let _ = updates
        .send(Update::Notice(format!("Task {}: {}", run.id, run.status)))
        .await;
    }
    Action::Task {
      session,
      generation,
      source,
      append,
      command,
    } => {
      let value =
        crate::task_management::execute(client, crate::task_command::Command::Task { command })
          .await?;
      let _ = updates
        .send(Update::TaskResult {
          session,
          generation,
          source,
          append,
          value,
        })
        .await;
    }
    Action::Resource {
      session,
      generation,
      navigation_revision,
      source,
      command,
    } => {
      if matches!(
        &command,
        crate::resource_command::ResourceCommand::Workflow(
          crate::plan_command::WorkflowCommand::Apply { .. }
        )
      ) {
        let client = client.clone();
        let updates = updates.clone();
        // Keep the command adapter available while the reviewed filesystem operation completes.
        tokio::spawn(async move {
          let result = crate::resource_backend::execute(&client, session.as_deref(), command).await;
          let (title, detail) = result.unwrap_or_else(|error| {
            (
              "Change apply result".into(),
              format!(
                "{error}. Inspect the checkpoint before trying again; no request was replayed."
              ),
            )
          });
          let _ = updates
            .send(Update::ResourceDetail {
              session,
              generation,
              navigation_revision,
              source,
              title,
              detail,
            })
            .await;
        });
        return Ok(());
      }
      let (title, detail) =
        crate::resource_backend::execute(client, session.as_deref(), command).await?;
      let _ = updates
        .send(Update::ResourceDetail {
          session,
          generation,
          navigation_revision,
          source,
          title,
          detail,
        })
        .await;
    }
    Action::ModelCatalog {
      session,
      generation,
    } => {
      let models = client
        .list_models(session.as_deref().unwrap_or_default())
        .await?;
      let _ = updates
        .send(Update::ModelCatalog {
          session,
          generation,
          models,
        })
        .await;
    }
    Action::ProviderCatalog { generation } => {
      let (providers, profiles) =
        tokio::try_join!(client.list_providers(), client.list_provider_profiles())?;
      let _ = updates
        .send(Update::ProviderCatalog {
          generation,
          providers,
          profiles,
        })
        .await;
    }
    Action::SkillsCatalog {
      session,
      generation,
      scope,
    } => {
      let skills = client
        .list_skills(&scope, session.as_deref().unwrap_or_default())
        .await?;
      let _ = updates
        .send(Update::SkillsCatalog {
          session,
          generation,
          scope,
          skills,
        })
        .await;
    }
    Action::WorkspaceDiff {
      session,
      generation,
      path,
    } => {
      let text = client.workspace_diff(&session, &path).await?;
      let _ = updates
        .send(Update::WorkspaceDiffText {
          session,
          generation,
          path,
          text,
        })
        .await;
    }
    Action::SaveState { .. } => {}
    Action::Run {
      session,
      generation,
      source,
      command,
    } => {
      let value = crate::run_management::execute(client, command).await?;
      let _ = updates
        .send(Update::RecoveryResult {
          session,
          generation,
          source,
          value,
        })
        .await;
    }
    Action::Approve { id, approved } => {
      client.approve(&id, approved).await?;
      let _ = updates.send(Update::ApprovalDecided(id)).await;
      let _ = updates
        .send(Update::Notice("Approval decision recorded.".into()))
        .await;
    }
  }
  Ok(())
}

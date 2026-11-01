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
      if let Some(run) = refresh_session(client, &id, generation, updates).await? {
        *stream = Some(watch(client, &run, generation, updates).await?);
      }
    }
    Action::Submit {
      session,
      prompt,
      generation,
    } => {
      let run = client
        .start_run(&session, &prompt, &uuid::Uuid::now_v7().to_string())
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

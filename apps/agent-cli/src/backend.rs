use agent_client::AgentClient;
use agent_protocol::v1::{Approval, Event, Run, Session};
use tokio::sync::mpsc;

use crate::{bootstrap::ClientModule, config::ResolvedConfig};

pub enum Action {
  Connect,
  NewSession,
  Select(String),
  Submit { session: String, prompt: String },
  Cancel(String),
  Approve { id: String, approved: bool },
}

pub enum Update {
  Connected(Vec<Session>),
  Disconnected(String),
  Selected(Session),
  Runs(Vec<Run>),
  Approvals {
    session: String,
    approvals: Vec<Approval>,
  },
  Event(Event),
  Notice(String),
  Submitted(String),
  ApprovalDecided(String),
}

pub fn spawn(config: ResolvedConfig) -> (mpsc::Sender<Action>, mpsc::Receiver<Update>) {
  let profiles = config.data_dir.join("browser-profiles").join("cli");
  let services = ClientModule::new(fluxdi::Shared::new(config));
  let (actions, mut receiver) = mpsc::channel(32);
  let (updates, output) = mpsc::channel(128);
  tokio::spawn(async move {
    let mut client = None;
    let mut selected = None;
    let mut browser: Option<agent_browser_runtime::RuntimeAttachment> = None;
    let mut stream: Option<tokio::task::JoinHandle<()>> = None;
    let mut refresh = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
      tokio::select! {
          action = receiver.recv() => {
              let Some(action) = action else { break };
              if matches!(action, Action::Connect) {
                  browser = None;
                  if let Some(task) = stream.take() { task.abort(); }
                  client = None;
                  match connect(&services).await {
                      Ok((connected, sessions)) => {
                          tracing::info!(operation = "client.connect", outcome = "authenticated", process_id = std::process::id(), session_count = sessions.len(), "Terminal client connected");
                          if agent_browser_runtime::runtime_available() {
                              match agent_browser_runtime::attach(connected.clone(), &profiles).await {
                                  Ok(attachment) => browser = Some(attachment),
                                  Err(error) => {
                                      tracing::warn!(operation = "browser.attach", outcome = "failed", process_id = std::process::id(), error_category = "runtime_attachment", "Terminal browser attachment failed");
                                      let _ = updates.send(Update::Notice(format!("BrowserRuntime unavailable: {error}"))).await;
                                  }
                              }
                          }
                          client = Some(connected);
                          let _ = updates.send(Update::Connected(sessions)).await;
                      }
                      Err(error) => {
                          tracing::warn!(operation = "client.connect", outcome = "failed", process_id = std::process::id(), error_category = "connection_or_handshake", "Terminal client connection failed");
                          let _ = updates.send(Update::Disconnected(error.to_string())).await;
                      }
                  }
                  continue;
              }
              let Some(client) = client.as_ref() else {
                  tracing::warn!(operation = "client.request", outcome = "rejected", process_id = std::process::id(), error_category = "disconnected", "Terminal request requires a connection");
                  let _ = updates.send(Update::Notice("Disconnected: Ctrl+R reconnects.".into())).await;
                  continue;
              };
              let operation = match &action {
                  Action::Connect => "client.connect",
                  Action::NewSession => "session.create",
                  Action::Select(_) => "session.select",
                  Action::Submit { .. } => "run.submit",
                  Action::Cancel(_) => "run.cancel",
                  Action::Approve { .. } => "approval.decide",
              };
              let session_id = match &action {
                  Action::Select(id) | Action::Submit { session: id, .. } => Some(id.clone()),
                  _ => selected.clone(),
              };
              let run_id = match &action {
                  Action::Cancel(id) => Some(id.clone()),
                  _ => None,
              };
              if let Err(error) = execute(client, action, &mut selected, &mut stream, &updates).await {
                  tracing::warn!(operation, outcome = "failed", process_id = std::process::id(), session_id = session_id.as_deref(), run_id = run_id.as_deref(), error_category = "request_failed", "Terminal request failed; no submission was replayed");
                  let _ = updates.send(Update::Notice(format!("Request failed: {error}. Unknown outcomes are not retried."))).await;
              }
          }
          _ = refresh.tick() => {
              if browser.as_ref().is_some_and(|attachment| attachment.is_finished()) {
                  let reason = browser.as_ref().and_then(|attachment| attachment.error())
                      .unwrap_or_else(|| "runtime stopped".into());
                  browser = None;
                  tracing::warn!(operation = "browser.disconnect", outcome = "disconnected", process_id = std::process::id(), session_id = selected.as_deref(), error_category = "runtime_stopped", "Terminal browser runtime disconnected");
                  let _ = updates.send(Update::Notice(format!("BrowserRuntime disconnected: {reason}. Browser actions are not replayed."))).await;
              }
              if let (Some(connected), Some(session)) = (&client, &selected)
                  && let Err(error) = refresh_session(connected, session, &updates).await {
                      tracing::warn!(operation = "session.refresh", outcome = "disconnected", process_id = std::process::id(), session_id = session.as_str(), error_category = "refresh_failed", "Terminal client disconnected after refresh failed");
                      let _ = updates.send(Update::Disconnected(error.to_string())).await;
                      client = None;
                      browser = None;
                      if let Some(task) = stream.take() { task.abort(); }
              }
          }
      }
    }
    tracing::info!(
      operation = "client.disconnect",
      outcome = "closed",
      process_id = std::process::id(),
      session_id = selected.as_deref(),
      "Terminal client subscriptions closing"
    );
    if let Some(task) = stream {
      task.abort();
    }
  });
  (actions, output)
}

async fn connect(services: &ClientModule) -> anyhow::Result<(AgentClient, Vec<Session>)> {
  let client = services.client().await?;
  let sessions = client.list_sessions().await?;
  Ok(((*client).clone(), sessions))
}

async fn execute(
  client: &AgentClient,
  action: Action,
  selected: &mut Option<String>,
  stream: &mut Option<tokio::task::JoinHandle<()>>,
  updates: &mpsc::Sender<Update>,
) -> anyhow::Result<()> {
  match action {
    Action::Connect => {}
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
    Action::Select(id) => {
      *selected = Some(id.clone());
      if let Some(task) = stream.take() {
        task.abort();
      }
      if let Some(run) = refresh_session(client, &id, updates).await? {
        *stream = Some(watch(client, &run, updates).await?);
      }
    }
    Action::Submit { session, prompt } => {
      let run = client
        .start_run(&session, &prompt, &uuid::Uuid::now_v7().to_string())
        .await?;
      let _ = updates.send(Update::Submitted(prompt)).await;
      let _ = updates
        .send(Update::Notice(format!("Submitted task {}", run.id)))
        .await;
      refresh_session(client, &session, updates).await?;
      if let Some(task) = stream.take() {
        task.abort();
      }
      *stream = Some(watch(client, &run.id, updates).await?);
    }
    Action::Cancel(id) => {
      let run = client.cancel_run(&id).await?;
      let _ = updates
        .send(Update::Notice(format!("Task {}: {}", run.id, run.status)))
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

async fn refresh_session(
  client: &AgentClient,
  session: &str,
  updates: &mpsc::Sender<Update>,
) -> anyhow::Result<Option<String>> {
  let runs = client.list_runs(session).await?;
  let active = runs
    .iter()
    .rev()
    .find(|run| {
      matches!(
        run.status.as_str(),
        "accepted" | "running" | "awaiting_approval"
      )
    })
    .map(|run| run.id.clone());
  let batches = futures::future::try_join_all(
    runs
      .iter()
      .filter(|run| run.status == "awaiting_approval")
      .map(|run| client.list_approvals(&run.id)),
  )
  .await?;
  let approvals = batches
    .into_iter()
    .flatten()
    .filter(|approval| approval.status == "pending")
    .collect();
  let _ = updates.send(Update::Runs(runs)).await;
  let _ = updates
    .send(Update::Approvals {
      session: session.to_owned(),
      approvals,
    })
    .await;
  Ok(active)
}

async fn watch(
  client: &AgentClient,
  run: &str,
  updates: &mpsc::Sender<Update>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
  let mut events = client.watch_run(run, 0).await?;
  let updates = updates.clone();
  let run_id = run.to_owned();
  Ok(tokio::spawn(async move {
    loop {
      match events.message().await {
        Ok(Some(event)) => {
          if updates.send(Update::Event(event)).await.is_err() {
            break;
          }
        }
        Ok(None) => break,
        Err(error) => {
          tracing::warn!(
            operation = "run.watch",
            outcome = "disconnected",
            process_id = std::process::id(),
            run_id,
            status_code = ?error.code(),
            "Terminal run event stream interrupted"
          );
          let _ = updates
            .send(Update::Notice(format!(
              "Event stream interrupted: {error}. Task state remains in agentd."
            )))
            .await;
          break;
        }
      }
    }
  }))
}

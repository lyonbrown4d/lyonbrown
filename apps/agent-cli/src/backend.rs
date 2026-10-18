use agent_client::AgentClient;
use agent_protocol::v1::{Approval, Event, Run, Session};
use tokio::sync::mpsc;

use crate::{
  bootstrap::ClientModule,
  computer::{ComputerStatus, LocalCommand},
  computer_backend::ComputerOwner,
  config::ResolvedConfig,
  session_backend::{refresh_session, watch},
};
use agent_computer_runtime::{ComputerHostFactory, NativeComputerHost};
use std::sync::Arc;

pub enum Action {
  Connect,
  Computer {
    session: Option<String>,
    command: LocalCommand,
  },
  Shutdown(tokio::sync::oneshot::Sender<()>),
  NewSession,
  Select(String),
  Submit {
    session: String,
    prompt: String,
  },
  Cancel(String),
  Approve {
    id: String,
    approved: bool,
  },
}

pub enum Update {
  Computer {
    status: ComputerStatus,
    detail: String,
  },
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
  spawn_with_computer_factory(config, Arc::new(NativeComputerHost))
}

/// Embed a host factory; native selection and execution remain behind its port.
pub fn spawn_with_computer_factory(
  config: ResolvedConfig,
  factory: Arc<dyn ComputerHostFactory>,
) -> (mpsc::Sender<Action>, mpsc::Receiver<Update>) {
  let profiles = config.data_dir.join("browser-profiles").join("cli");
  let services = ClientModule::new(fluxdi::Shared::new(config));
  let (actions, mut receiver) = mpsc::channel(32);
  let (updates, output) = mpsc::channel(128);
  tokio::spawn(async move {
    let mut computer = ComputerOwner::new(factory);
    let _ = updates.send(computer.snapshot()).await;
    let mut client = None;
    let mut selected = None;
    let mut browser: Option<agent_browser_runtime::RuntimeAttachment> = None;
    let mut stream: Option<tokio::task::JoinHandle<()>> = None;
    let mut refresh = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
      tokio::select! {
          action = receiver.recv() => {
              let Some(action) = action else { break };
              if let Action::Shutdown(done) = action {
                  computer.disconnected();
                  drop(browser.take());
                  if let Some(task) = stream.take() { task.abort(); }
                  let _ = done.send(());
                  break;
              }
              if let Action::Computer { session, command } = action {
                  if client.is_some() && let Some(update) = computer.pending(session.as_deref(), &command) {
                    let _ = updates.send(update).await;
                  }
                  computer.execute(client.as_ref(), session.as_deref(), command).await;
                  let _ = updates.send(computer.snapshot()).await;
                  continue;
              }
              if matches!(action, Action::Connect) {
                  computer.disconnected();
                  let _ = updates.send(computer.snapshot()).await;
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
                  Action::Computer { .. } | Action::Shutdown(_) => "computer.control",
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
              if computer.poll() { let _ = updates.send(computer.snapshot()).await; }
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
                      computer.disconnected();
                      let _ = updates.send(computer.snapshot()).await;
                      client = None;
                      browser = None;
                      if let Some(task) = stream.take() { task.abort(); }
              }
          }
      }
    }
    computer.disconnected();
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

use agent_client::AgentClient;
use agent_client::local_state::LocalStateStore;
use agent_protocol::v1::{Approval, Event, Run, Session};
use tokio::sync::mpsc;

use crate::{
  bootstrap::ClientModule,
  computer::{ComputerStatus, LocalCommand},
  computer_backend::ComputerOwner,
  config::ResolvedConfig,
  session_backend::refresh_session,
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
  Select {
    session: String,
    generation: u64,
  },
  Submit {
    session: String,
    prompt: String,
    generation: u64,
  },
  Cancel(String),
  Approve {
    id: String,
    approved: bool,
  },
  Task {
    session: Option<String>,
    generation: u64,
    source: Option<String>,
    append: bool,
    command: crate::task_command::TaskCommand,
  },
  Run {
    session: Option<String>,
    generation: u64,
    source: String,
    command: crate::run_command::RunCommand,
  },
  SaveState {
    node_id: String,
    state: crate::local_state::ClientState,
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
  Runs {
    session: String,
    generation: u64,
    runs: Vec<Run>,
  },
  Approvals {
    session: String,
    generation: u64,
    approvals: Vec<Approval>,
  },
  Event {
    generation: u64,
    event: Event,
  },
  Notice(String),
  Submitted {
    session: String,
    prompt: String,
    generation: u64,
  },
  ApprovalDecided(String),
  TaskResult {
    session: Option<String>,
    generation: u64,
    source: Option<String>,
    append: bool,
    value: serde_json::Value,
  },
  RecoveryResult {
    session: Option<String>,
    generation: u64,
    source: String,
    value: serde_json::Value,
  },
  LocalState {
    node_id: Option<String>,
    state: Option<crate::local_state::ClientState>,
    warning: Option<String>,
    preserve: bool,
  },
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
  let state_root = config.data_dir.clone();
  let state_endpoint = config.endpoint.clone();
  let services = ClientModule::new(fluxdi::Shared::new(config));
  let (actions, mut receiver) = mpsc::channel(32);
  let (updates, output) = mpsc::channel(128);
  tokio::spawn(async move {
    let mut computer = ComputerOwner::new(factory);
    let _ = updates.send(computer.snapshot()).await;
    let mut client = None;
    let mut state_store: Option<LocalStateStore> = None;
    let mut state_node_id: Option<String> = None;
    let mut selected = None;
    let mut selected_generation = 0;
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
              if let Action::SaveState { node_id, state } = action {
                  if state_node_id.as_deref() == Some(node_id.as_str())
                    && let Some(store) = state_store.take() {
                      match crate::state_backend::save(store, state).await {
                          Ok((store, Ok(()))) => state_store = Some(store),
                          Ok((store, Err(error))) => {
                              state_store = Some(store);
                              let _ = updates.send(Update::Notice(format!("TUI state could not be saved: {error}"))).await;
                          }
                          Err(error) => {
                              let _ = updates.send(Update::Notice(format!("TUI state save worker failed: {error}"))).await;
                          }
                      }
                  }
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
                          client = Some(connected.clone());
                          let _ = updates.send(Update::Connected(sessions)).await;
                          match connected.node().await {
                              Ok(node) if state_node_id.as_deref() != Some(node.id.as_str()) => {
                                  state_node_id = Some(node.id.clone());
                                  state_store = None;
                                  match crate::state_backend::load(state_root.clone(), state_endpoint.clone(), node.id.clone()).await {
                                      Ok((store, state)) => {
                                          state_store = Some(store);
                                          let _ = updates.send(Update::LocalState { node_id: Some(node.id), state, warning: None, preserve: false }).await;
                                      }
                                      Err(error) => {
                                              let _ = updates.send(Update::LocalState {
                                                  node_id: Some(node.id),
                                              state: None,
                                              warning: Some(format!("Saved TUI state is corrupt or unavailable; it was left untouched. Recover or remove the client-state file to resume saving: {error}")),
                                                  preserve: false,
                                          }).await;
                                      }
                                  }
                              }
                              Ok(node) => {
                                  let _ = updates.send(Update::LocalState { node_id: Some(node.id), state: None, warning: None, preserve: true }).await;
                              }
                              Err(error) => {
                                  let _ = updates.send(Update::LocalState { node_id: None, state: None, warning: Some(format!("TUI state persistence is unavailable because daemon identity could not be verified: {error}")), preserve: false }).await;
                              }
                          }
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
                  Action::Select { .. } => "session.select",
                  Action::Submit { .. } => "run.submit",
                  Action::Cancel(_) => "run.cancel",
                  Action::Approve { .. } => "approval.decide",
                  Action::Task { .. } => "task.manage",
                  Action::Run { .. } => "run.recover",
                  Action::SaveState { .. } => "client.state.save",
              };
              let session_id = match &action {
                  Action::Select { session: id, .. } | Action::Submit { session: id, .. } => Some(id.clone()),
                  _ => selected.clone(),
              };
              let run_id = match &action {
                  Action::Cancel(id) => Some(id.clone()),
                  _ => None,
              };
              if let Action::Select { generation, .. } = &action {
                  selected_generation = *generation;
              }
              if let Err(error) = crate::action_backend::execute(client, action, &mut selected, &mut selected_generation, &mut stream, &updates).await {
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
                  && let Err(error) = refresh_session(connected, session, selected_generation, &updates).await {
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

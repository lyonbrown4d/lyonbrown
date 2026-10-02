use std::sync::Arc;

use agent_app::AgentClient;
use agent_client::{NativeAgentClient, RemoteState};
use anyhow::{Context, Result};
use tokio::sync::watch;

use super::super::{ApprovalRequest, Locale, RenderContext, TuiState, terminal::TerminalSession};
use super::language::{self, LanguageCommand};

pub struct Runtime {
  pub state: TuiState,
  pub client: Option<Arc<dyn AgentClient>>,
  pub snapshots: watch::Receiver<agent_app::RuntimeSnapshot>,
  pub remote_state: Option<watch::Receiver<RemoteState>>,
  pub approval: Option<ApprovalRequest>,
  pub approval_scroll: u16,
  pub approval_max_scroll: u16,
  pub conversation_scroll: u16,
  pub conversation_max_scroll: u16,
  pub locale: Locale,
}

impl Runtime {
  pub const fn locale(&self) -> Locale {
    self.locale
  }

  pub fn apply_language_command(&mut self, command: LanguageCommand) -> bool {
    language::apply(self, command)
  }

  pub fn render_context(
    &self,
    setup_command_pending: bool,
    setup_exit_pending: bool,
  ) -> RenderContext {
    RenderContext {
      locale: self.locale,
      setup_command_pending,
      setup_exit_pending,
      ..RenderContext::default()
    }
  }
}

pub async fn run_client(client: Arc<dyn AgentClient>) -> Result<()> {
  let snapshots = client.subscribe();
  run_client_with_state(client, snapshots, None).await
}

pub async fn run_native_client(client: NativeAgentClient) -> Result<()> {
  let remote_state = client.subscribe_remote_state();
  let client: Arc<dyn AgentClient> = Arc::new(client);
  let snapshots = client.subscribe();
  run_client_with_state(client, snapshots, Some(remote_state)).await
}

async fn run_client_with_state(
  client: Arc<dyn AgentClient>,
  snapshots: watch::Receiver<agent_app::RuntimeSnapshot>,
  remote_state: Option<watch::Receiver<RemoteState>>,
) -> Result<()> {
  let state = TuiState::default();
  let mut terminal = match TerminalSession::enter() {
    Ok(terminal) => terminal,
    Err(error) => return Err(error).context("failed to initialize terminal interface"),
  };
  let mut runtime = Runtime {
    state,
    client: Some(client),
    snapshots,
    remote_state,
    approval: None,
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
    locale: Locale::system_default(),
  };

  let result = super::run_event_loop(&mut terminal, &mut runtime).await;
  let close = shutdown(&mut runtime).await;
  result?;
  close
}

pub async fn shutdown(runtime: &mut Runtime) -> Result<()> {
  runtime.client.take();
  Ok(())
}

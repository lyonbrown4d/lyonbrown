use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use agent_app::{AgentClient, AgentClientError, AgentCommand, RuntimePhase, RuntimeSnapshot};
use agent_cli::tui::runner::Runtime;
use agent_cli::tui::{LanguageCommand, Locale, TuiState, apply_remote_state, render_with_context};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{Terminal, backend::TestBackend};
use tokio::sync::watch;

#[test]
fn runtime_language_selection_is_local_and_drives_render_context() {
  let (mut runtime, calls) = runtime(Locale::EnUs);
  let transcript_before = runtime.state.transcript().to_vec();

  assert!(runtime.apply_language_command(LanguageCommand::Toggle));

  assert_eq!(runtime.locale(), Locale::ZhCn);
  assert_eq!(runtime.render_context(false, false).locale, Locale::ZhCn);
  assert_eq!(runtime.state.transcript(), transcript_before);
  assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn invalid_language_usage_is_localized_without_translating_locale_ids() {
  for (locale, expected) in [(Locale::EnUs, "usage:"), (Locale::ZhCn, "用法：")] {
    let (mut runtime, calls) = runtime(locale);
    let transcript_before = runtime.state.transcript().to_vec();

    assert!(runtime.apply_language_command(LanguageCommand::Invalid));
    runtime.state.sync_snapshot(&RuntimeSnapshot::default());

    let notice = runtime.state.client_local_notice().unwrap();
    assert!(notice.contains(expected), "{notice}");
    assert!(notice.contains("/language [en-US|zh-CN]"), "{notice}");
    assert_eq!(runtime.state.transcript(), transcript_before);
    let output = screen(&runtime);
    assert!(
      output.replace(' ', "").contains(&notice.replace(' ', "")),
      "{notice}\n{output}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
  }
}

#[test]
fn editing_or_successful_language_selection_clears_a_local_notice() {
  let (mut runtime, _) = runtime(Locale::EnUs);
  runtime.apply_language_command(LanguageCommand::Invalid);

  runtime.state.apply_input_key(KeyCode::Char('x'));

  assert_eq!(runtime.state.client_local_notice(), None);
  runtime.apply_language_command(LanguageCommand::Invalid);
  runtime.apply_language_command(LanguageCommand::Toggle);
  assert_eq!(runtime.state.client_local_notice(), None);
  runtime.apply_language_command(LanguageCommand::Invalid);
  runtime.apply_language_command(LanguageCommand::Set(Locale::ZhCn));
  assert_eq!(runtime.state.client_local_notice(), None);
  runtime.apply_language_command(LanguageCommand::Invalid);
  assert!(!runtime.apply_language_command(LanguageCommand::NotLanguage));
  assert_eq!(runtime.state.client_local_notice(), None);
}

fn screen(runtime: &Runtime) -> String {
  let backend = TestBackend::new(90, 16);
  let mut terminal = Terminal::new(backend).unwrap();
  terminal
    .draw(|frame| render_with_context(frame, &runtime.state, runtime.render_context(false, false)))
    .unwrap();
  let buffer = terminal.backend().buffer();
  (0..buffer.area.height)
    .map(|y| {
      (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn runtime(locale: Locale) -> (Runtime, Arc<AtomicUsize>) {
  let client = Arc::new(CountingClient::new());
  let calls = Arc::clone(&client.calls);
  let snapshots = client.subscribe();
  let mut state = TuiState::default();
  apply_remote_state(
    &mut state,
    &RemoteState {
      connection: ConnectionState::Connected {
        server_instance_id: "locale-daemon".into(),
      },
      snapshot: Some(VersionedRemoteSnapshot {
        server_instance_id: "locale-daemon".into(),
        revision: 1,
        snapshot: RuntimeSnapshot {
          phase: RuntimePhase::Ready,
          ..RuntimeSnapshot::default()
        },
      }),
    },
  );
  (
    Runtime {
      state,
      client: Some(client),
      snapshots,
      remote_state: None,
      approval: None,
      approval_scroll: 0,
      approval_max_scroll: 0,
      conversation_scroll: 0,
      conversation_max_scroll: 0,
      locale,
    },
    calls,
  )
}

struct CountingClient {
  snapshots: watch::Sender<RuntimeSnapshot>,
  calls: Arc<AtomicUsize>,
}

impl CountingClient {
  fn new() -> Self {
    let (snapshots, _) = watch::channel(RuntimeSnapshot::default());
    Self {
      snapshots,
      calls: Arc::new(AtomicUsize::new(0)),
    }
  }
}

#[async_trait]
impl AgentClient for CountingClient {
  async fn execute(&self, _command: AgentCommand) -> Result<(), AgentClientError> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    Ok(())
  }

  async fn execute_for_server(
    &self,
    _expected_server_instance_id: &str,
    _command: AgentCommand,
  ) -> Result<(), AgentClientError> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    Ok(())
  }

  fn snapshot(&self) -> RuntimeSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<RuntimeSnapshot> {
    self.snapshots.subscribe()
  }
}

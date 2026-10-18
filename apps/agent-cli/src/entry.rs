use crate::{
  backend::{self, Action},
  config::{Config, ResolvedConfig},
  model::{App, Connection},
  terminal,
  theme::Theme,
  update, view,
};
use clap::{CommandFactory, FromArgMatches, parser::ValueSource};
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use tokio::sync::mpsc;

pub fn run() -> anyhow::Result<std::process::ExitCode> {
  if let Some(code) = agent_browser_runtime::dispatch_entry()? {
    return Ok(code);
  }
  let matches = Config::command().get_matches();
  let explicit_endpoint = matches.value_source("endpoint") != Some(ValueSource::DefaultValue);
  let config = Config::from_arg_matches(&matches)?.resolve(explicit_endpoint)?;
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()?;
  runtime.block_on(run_terminal(config))?;
  Ok(std::process::ExitCode::SUCCESS)
}

async fn run_terminal(mut config: ResolvedConfig) -> anyhow::Result<()> {
  let _logs = agent_telemetry::init_with_options(
    &agent_telemetry::LogOptions {
      directory: config.settings.log_directory.clone(),
      retained_files: config.settings.log_retention,
      stderr: false,
    },
    "agent-cli",
    &std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
  )?;
  if let Some(command) = config.command.take() {
    let client = crate::bootstrap::connect_client(&config).await?;
    let value = crate::task_management::execute(&client, command).await?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    return Ok(());
  }
  tracing::info!(operation = "client.start", "terminal client starting");
  let theme = Theme::resolve(config.theme, std::env::var("COLORFGBG").ok().as_deref());
  let (actions, mut updates) = backend::spawn(config);
  actions.send(Action::Connect).await?;
  let mut terminal = terminal::TerminalGuard::enter()?;
  let mut events = EventStream::new();
  let mut app = App {
    connection: Connection::Connecting,
    ..App::default()
  };
  let stop = terminal::shutdown();
  tokio::pin!(stop);
  loop {
    terminal.draw(|frame| view::render(frame, &app, theme))?;
    tokio::select! {
        _ = &mut stop => break,
        update = updates.recv() => {
            let Some(update) = update else { break };
            let connected = matches!(update, backend::Update::Connected(_));
            update::apply(&mut app, update);
            if connected && let Some(session) = app.selected_session() {
                send(&actions, Action::Select(session.id.clone()))?;
            }
        }
        event = events.next() => match event {
            Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('c') | KeyCode::Char('d') => break,
                        KeyCode::Char('n') => send(&actions, Action::NewSession)?,
                        KeyCode::Char('r') => {
                            app.connection = Connection::Connecting;
                            send(&actions, Action::Connect)?;
                        }
                        KeyCode::Char('x') => {
                            if let Some(id) = &app.active_run { send(&actions, Action::Cancel(id.clone()))?; }
                        }
                        KeyCode::Char('y') | KeyCode::Char('e') => {
                            if let Some(id) = app.approvals.first() {
                                send(&actions, Action::Approve { id: id.id.clone(), approved: key.code == KeyCode::Char('y') })?;
                            }
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char(character) => app.input.push(character),
                        KeyCode::Backspace => { app.input.pop(); }
                        KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) || key.modifiers.contains(KeyModifiers::SHIFT) => app.input.push('\n'),
                        KeyCode::Enter => {
                            if app.connection != Connection::Connected {
                                app.notice = "Disconnected: Ctrl+R reconnects. Your draft is preserved.".into();
                            } else if let Some(session) = app.selected_session() {
                                if !app.submitting && !app.input.trim().is_empty() {
                                    send(&actions, Action::Submit { session: session.id.clone(), prompt: app.input.clone() })?;
                                    app.submitting = true;
                                }
                            } else { app.notice = "Ctrl+N creates a session before sending.".into(); }
                        }
                        KeyCode::Tab => {
                            app.next_session();
                            if let Some(session) = app.selected_session() { send(&actions, Action::Select(session.id.clone()))?; }
                        }
                        KeyCode::PageDown => app.scroll = app.scroll.saturating_add(5),
                        KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(5),
                        _ => {}
                    }
                }
            }
            Some(Ok(Event::Paste(text))) => app.input.push_str(&text),
            Some(Err(error)) => return Err(error.into()),
            None => break,
            _ => {}
        }
    }
  }
  Ok(())
}

fn send(actions: &mpsc::Sender<Action>, action: Action) -> anyhow::Result<()> {
  actions
    .try_send(action)
    .map_err(|_| anyhow::anyhow!("Client request queue unavailable; reconnect before retrying"))
}

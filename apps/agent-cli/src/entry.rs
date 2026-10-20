use crate::{
  backend::{self, Action},
  config::{Config, ResolvedConfig},
  input::{self, send},
  model::{App, Connection},
  terminal,
  theme::Theme,
  update, view,
};
use clap::{CommandFactory, FromArgMatches, parser::ValueSource};
use crossterm::event::EventStream;
use futures::StreamExt;

pub fn run() -> anyhow::Result<std::process::ExitCode> {
  if let Some(code) = agent_browser_runtime::dispatch_entry()? {
    return Ok(code);
  }
  let matches = Config::command().get_matches();
  let explicit_endpoint = matches.value_source("endpoint") != Some(ValueSource::DefaultValue);
  let config = Config::from_arg_matches(&matches)?.resolve(explicit_endpoint)?;
  run_terminal(config)?;
  Ok(std::process::ExitCode::SUCCESS)
}

#[tokio::main]
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
    if let crate::task_command::Command::Computer { command } = command {
      return crate::computer_command::execute(&config, command).await;
    }
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
            Some(Ok(event)) => {
                if input::handle(&mut app, event, &actions)? { break; }
            }
            Some(Err(error)) => return Err(error.into()),
            None => break,
        }
    }
  }
  let (done, closed) = tokio::sync::oneshot::channel();
  if actions.send(Action::Shutdown(done)).await.is_ok() {
    let _ = closed.await;
  }
  Ok(())
}

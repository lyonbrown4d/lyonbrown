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
  let mut config = Config::from_arg_matches(&matches)?.resolve(explicit_endpoint)?;
  let _logs = agent_telemetry::init_with_options(
    &agent_telemetry::LogOptions {
      directory: config.settings.log_directory.clone(),
      retained_files: config.settings.log_retention,
      stderr: false,
    },
    "agent-cli",
    &std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
  )?;
  match config.command.take() {
    Some(crate::task_command::Command::Exec(args)) => {
      crate::exec::run(&config, args).map(std::process::ExitCode::from)
    }
    command => {
      config.command = command;
      run_terminal(config).map(std::process::ExitCode::from)
    }
  }
}

#[tokio::main]
async fn run_terminal(mut config: ResolvedConfig) -> anyhow::Result<u8> {
  if let Some(command) = config.command.take() {
    if let crate::task_command::Command::Computer { command } = command {
      crate::computer_command::execute(&config, command).await?;
      return Ok(0);
    }
    let client = crate::bootstrap::connect_client(&config).await?;
    let value = crate::task_management::execute(&client, command).await?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    return Ok(0);
  }
  tracing::info!(operation = "client.start", "terminal client starting");
  let theme = Theme::resolve(config.theme, std::env::var("COLORFGBG").ok().as_deref());
  let (actions, mut updates) = backend::spawn(config);
  actions.send(Action::Connect).await?;
  let mut terminal = terminal::TerminalGuard::enter()?;
  let mut events = EventStream::new();
  let mut app = App {
    connection: Connection::Connecting,
    state_loaded: false,
    ..App::default()
  };
  let stop = terminal::shutdown();
  tokio::pin!(stop);
  let mut save_state = tokio::time::interval(std::time::Duration::from_millis(300));
  let mut last_saved = None;
  loop {
    let completed = terminal.draw(|frame| view::render(frame, &app, theme))?;
    app.viewport = completed.area.height;
    app.viewport_width = completed.area.width;
    tokio::select! {
        _ = &mut stop => break,
        update = updates.recv() => {
            let Some(update) = update else { break };
            let connected = matches!(update, backend::Update::Connected(_));
            let restored = matches!(update, backend::Update::LocalState { .. });
            update::apply(&mut app, update);
            if connected && let Some(session) = app.selected_session() {
                send(&actions, Action::Select { session: session.id.clone(), generation: app.generation })?;
            }
            if restored {
                if let Some(session) = app.selected_session() {
                    send(&actions, Action::Select { session: session.id.clone(), generation: app.generation })?;
                }
                if app.router.current() == crate::model::ContextPanel::Tasks {
                    crate::local_command::load_tasks(&app, &actions)?;
                }
            }
        }
        _ = save_state.tick() => {
            if app.state_loaded && let Some(node_id) = app.persistence_node_id.clone() {
                let state = app.client_state();
                if last_saved.as_ref() != Some(&state) {
                    send(&actions, Action::SaveState { node_id, state: state.clone() })?;
                    last_saved = Some(state);
                }
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
  if app.state_loaded
    && let Some(node_id) = app.persistence_node_id.clone()
  {
    let state = app.client_state();
    actions.send(Action::SaveState { node_id, state }).await?;
  }
  let (done, closed) = tokio::sync::oneshot::channel();
  if actions.send(Action::Shutdown(done)).await.is_ok() {
    let _ = closed.await;
  }
  Ok(0)
}

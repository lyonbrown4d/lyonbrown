//! Explicit client-owned selection and display state; authority stays in agentd.
use agent_computer_runtime::{ComputerHostFactory, DesktopScope, DesktopTarget};
use clap::{Args, Subcommand};
use thiserror::Error;

#[derive(Subcommand)]
pub enum ComputerCommand {
  /// List local targets without connecting to a daemon or capturing pixels.
  Targets,
  /// Keep one explicitly selected target attached until interrupted.
  Attach(Attach),
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["window", "display"])))]
pub struct Attach {
  #[arg(long, value_parser = session_id)]
  pub session: String,
  #[arg(long, requires = "process_id")]
  pub window: Option<u32>,
  /// Process identity shown by computer targets; prevents selecting a reused window ID.
  #[arg(long, requires = "window", conflicts_with = "display", value_parser = clap::value_parser!(u32).range(1..))]
  pub process_id: Option<u32>,
  #[arg(long)]
  pub display: Option<u32>,
}
impl Attach {
  pub fn scope(&self) -> Result<DesktopScope, ComputerError> {
    match (self.window, self.display, self.process_id) {
      (Some(id), None, Some(pid)) if pid > 0 => Ok(DesktopScope::Window(id)),
      (None, Some(id), None) => Ok(DesktopScope::Display(id)),
      _ => Err(ComputerError::InvalidCommand),
    }
  }
  /// CLI consent includes both native window and process identities from discovery.
  pub async fn resolve_target(
    &self,
    factory: &dyn ComputerHostFactory,
  ) -> Result<DesktopTarget, ComputerError> {
    let target = resolve_target(factory, &self.scope()?).await?;
    if matches!(target.scope, DesktopScope::Window(_)) && target.process_id != self.process_id {
      return Err(ComputerError::TargetMissing);
    }
    Ok(target)
  }
}
fn session_id(value: &str) -> Result<String, String> {
  uuid::Uuid::parse_str(value)
    .ok()
    .filter(|id| !id.is_nil())
    .map(|id| id.to_string())
    .ok_or_else(|| "session must be a non-nil UUID".into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalCommand {
  Targets,
  Attach(DesktopScope),
  Detach,
  Status,
}
/// A recognized prefix never falls through to a model, including invalid syntax.
pub fn parse_local(input: &str) -> Option<Result<LocalCommand, ComputerError>> {
  let words = input.split_whitespace().collect::<Vec<_>>();
  if words.first().copied() != Some("/computer") {
    return None;
  }
  Some(match words.as_slice() {
    [_, "targets"] => Ok(LocalCommand::Targets),
    [_, "status"] => Ok(LocalCommand::Status),
    [_, "detach"] => Ok(LocalCommand::Detach),
    [_, "attach", kind, id] => id
      .parse::<u32>()
      .map_err(|_| ComputerError::InvalidCommand)
      .and_then(|id| match *kind {
        "window" => Ok(LocalCommand::Attach(DesktopScope::Window(id))),
        "display" => Ok(LocalCommand::Attach(DesktopScope::Display(id))),
        _ => Err(ComputerError::InvalidCommand),
      }),
    _ => Err(ComputerError::InvalidCommand),
  })
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ComputerError {
  #[error("{0}")]
  Native(&'static str),
  #[error("ComputerRuntime unavailable in this client build")]
  Unavailable,
  #[error("Run /computer targets before selecting a desktop target")]
  CatalogRequired,
  #[error("Selected desktop target is absent from the current catalog")]
  TargetMissing,
  #[error("Detach the current computer attachment before selecting another session or target")]
  AlreadyAttached,
  #[error("Select a connected daemon session before attaching a computer target")]
  SessionRequired,
  #[error("ComputerRuntime disconnected; actions were not replayed")]
  Disconnected,
  #[error("ComputerRuntime shutdown failed; native work may still be draining")]
  Shutdown,
  #[error(
    "Use /computer targets, /computer attach window ID|display ID, /computer detach, or /computer status"
  )]
  InvalidCommand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComputerStatus {
  Unavailable,
  Detached,
  Attaching {
    session: String,
    scope: DesktopScope,
  },
  Attached {
    session: String,
    scope: DesktopScope,
  },
  Stopped {
    session: Option<String>,
    reason: ComputerError,
  },
}
impl ComputerStatus {
  pub fn summary(&self) -> String {
    match self {
      Self::Unavailable => "Computer: unavailable in this build".into(),
      Self::Detached => "Computer: detached".into(),
      Self::Attaching { session, scope } => format!(
        "Computer: attaching · session {session} · {}",
        scope_text(scope)
      ),
      Self::Attached { session, scope } => format!(
        "Computer: attached · session {session} · {}",
        scope_text(scope)
      ),
      Self::Stopped { session, reason } => format!(
        "Computer: {reason}{}",
        session
          .as_ref()
          .map(|id| format!(" · session {id}"))
          .unwrap_or_default()
      ),
    }
  }
}
pub fn scope_text(scope: &DesktopScope) -> String {
  match scope {
    DesktopScope::Window(id) => format!("window {id}"),
    DesktopScope::Display(id) => format!("display {id}"),
  }
}
pub fn target_text(target: &DesktopTarget) -> String {
  // Native titles are untrusted display text; remove terminal controls and bound it.
  let label: String = target
    .label
    .chars()
    .filter(|ch| !ch.is_control())
    .take(120)
    .collect();
  format!("{} · {label}", scope_text(&target.scope))
}
pub async fn targets(
  factory: &dyn ComputerHostFactory,
) -> Result<Vec<DesktopTarget>, ComputerError> {
  if !factory.available() {
    return Err(ComputerError::Unavailable);
  }
  factory
    .targets()
    .await
    .map_err(|error| ComputerError::Native(agent_computer_runtime::computer_error_message(&error)))
}
pub async fn resolve_target(
  factory: &dyn ComputerHostFactory,
  scope: &DesktopScope,
) -> Result<DesktopTarget, ComputerError> {
  targets(factory)
    .await?
    .into_iter()
    .find(|target| &target.scope == scope)
    .ok_or(ComputerError::TargetMissing)
}

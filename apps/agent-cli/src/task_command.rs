use agent_protocol::v1;
use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum Command {
  /// Attach this interactive client to an explicitly selected native desktop target.
  Computer {
    #[command(subcommand)]
    command: crate::computer::ComputerCommand,
  },
  /// Manage durable tasks through the authenticated Master daemon.
  Task {
    #[command(subcommand)]
    command: TaskCommand,
  },
}

#[derive(Subcommand)]
pub enum TaskCommand {
  List {
    #[arg(long, value_parser = identifier)]
    session: String,
    #[arg(long, value_parser = identifier)]
    after: Option<String>,
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=500))]
    page_size: u32,
  },
  Get {
    #[arg(value_parser = identifier)]
    task_id: String,
  },
  Reconcile(RemoteTask),
  Cancel(RemoteTask),
  Accept {
    #[command(flatten)]
    task: RemoteTask,
    /// Explicitly accept or reject the Worker result.
    #[arg(long, action = clap::ArgAction::Set, required = true)]
    accepted: bool,
  },
  Dispatch(Dispatch),
}

#[derive(Args)]
pub struct Target {
  /// Existing paired Worker identity; pairing trust stays in agentd.
  #[arg(long, value_parser = identifier)]
  pub node_id: String,
  /// Credential-free HTTPS origin of the paired Worker.
  #[arg(long)]
  pub node_endpoint: String,
}

impl Target {
  pub fn node(&self) -> v1::Node {
    v1::Node {
      id: self.node_id.clone(),
      endpoint: self.node_endpoint.clone(),
      ..Default::default()
    }
  }
}

#[derive(Args)]
pub struct RemoteTask {
  #[arg(value_parser = identifier)]
  pub task_id: String,
  #[command(flatten)]
  pub target: Target,
}

#[derive(Args)]
pub struct Dispatch {
  #[command(flatten)]
  pub target: Target,
  #[arg(long, value_parser = identifier)]
  pub session: String,
  /// Existing active run in the same session.
  #[arg(long, value_parser = identifier)]
  pub parent_run: String,
  /// Stable UUID; use get/reconcile with this identity after a lost response.
  #[arg(long, value_parser = uuid)]
  pub task_id: String,
  #[arg(long, value_parser = uuid)]
  pub attempt_id: String,
  #[arg(long, value_parser = identifier)]
  pub idempotency_key: String,
  #[arg(long)]
  pub instructions: String,
  #[arg(long, value_parser = ["workspace.read", "workspace.write.approval"], num_args = 1..=2)]
  pub capability: Vec<String>,
  #[arg(long, default_value_t = 3600, value_parser = clap::value_parser!(u32).range(1..=86400))]
  pub deadline_seconds: u32,
}

fn identifier(value: &str) -> Result<String, String> {
  if value.is_empty()
    || value.len() > 256
    || !value
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
  {
    return Err("identifier must contain 1..=256 ASCII letters, digits, -, _, : or .".into());
  }
  Ok(value.into())
}

fn uuid(value: &str) -> Result<String, String> {
  uuid::Uuid::parse_str(value)
    .map(|id| id.to_string())
    .map_err(|_| "task and attempt identifiers must be UUIDs".into())
}

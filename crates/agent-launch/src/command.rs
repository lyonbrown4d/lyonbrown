use crate::{DaemonExit, DaemonSpawner, SpawnError, SpawnMonitor, SpawnRequest};
use async_trait::async_trait;
use std::ffi::OsString;
use std::process::{Command, Stdio};
use tokio::io::AsyncReadExt;

const MAX_CAPTURED_STDERR_BYTES: usize = 4_096;

#[derive(Debug, Clone)]
pub struct CommandDaemonSpawner {
  program: OsString,
  fixed_arguments: Vec<OsString>,
}

impl CommandDaemonSpawner {
  pub fn new<P, I, A>(program: P, fixed_arguments: I) -> Self
  where
    P: Into<OsString>,
    I: IntoIterator<Item = A>,
    A: Into<OsString>,
  {
    Self {
      program: program.into(),
      fixed_arguments: fixed_arguments.into_iter().map(Into::into).collect(),
    }
  }

  pub fn command_for(&self, request: &SpawnRequest) -> Command {
    let mut command = Command::new(&self.program);
    command
      .args(&self.fixed_arguments)
      .arg("--workspace")
      .arg(request.canonical_workspace())
      .arg("--runtime-config-fingerprint")
      .arg(request.runtime_config_fingerprint())
      .current_dir(request.canonical_workspace())
      .env_remove("AGENT_AUTH_TOKEN")
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::piped());
    command
  }
}

#[async_trait]
impl DaemonSpawner for CommandDaemonSpawner {
  async fn spawn(&self, request: &SpawnRequest) -> Result<SpawnMonitor, SpawnError> {
    let command = self.command_for(request);
    let mut command = tokio::process::Command::from(command);
    let mut child = command.spawn()?;
    let stderr = child
      .stderr
      .take()
      .ok_or_else(|| SpawnError::Io(std::io::Error::other("daemon stderr pipe was not created")))?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
      let drain = tokio::spawn(drain_stderr(stderr));
      let status = child.wait().await;
      let stderr = drain
        .await
        .unwrap_or_else(|error| format!("failed to collect daemon stderr: {error}"));
      let (exit_code, stderr) = match status {
        Ok(status) => (status.code(), stderr),
        Err(error) => (None, append_diagnostic(stderr, &error.to_string())),
      };
      let _ = sender.send(DaemonExit::new(exit_code, stderr));
    });
    Ok(SpawnMonitor::from_receiver(receiver))
  }
}

async fn drain_stderr(mut stderr: tokio::process::ChildStderr) -> String {
  let mut captured = Vec::with_capacity(MAX_CAPTURED_STDERR_BYTES);
  let mut buffer = [0_u8; 8_192];
  loop {
    match stderr.read(&mut buffer).await {
      Ok(0) => break,
      Ok(read) => {
        let remaining = MAX_CAPTURED_STDERR_BYTES.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..read.min(remaining)]);
      }
      Err(error) => {
        return append_diagnostic(
          String::from_utf8_lossy(&captured).into(),
          &error.to_string(),
        );
      }
    }
  }
  String::from_utf8_lossy(&captured).into_owned()
}

fn append_diagnostic(mut captured: String, diagnostic: &str) -> String {
  if !captured.is_empty() {
    captured.push(' ');
  }
  captured.push_str(diagnostic);
  captured
}

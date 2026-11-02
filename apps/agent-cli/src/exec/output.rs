use std::{fs::File, io::Write as _};

use agent_protocol::v1;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::exec_command::OutputFormat;

struct Packet {
  bytes: Vec<u8>,
  flushed: oneshot::Sender<std::io::Result<()>>,
}

pub(super) struct Output {
  format: OutputFormat,
  sender: mpsc::Sender<Packet>,
}

impl Output {
  pub fn new(format: OutputFormat) -> Result<Self> {
    let mut stream = stdout_file()?;
    let (sender, mut receiver) = mpsc::channel::<Packet>(1);
    // A pipe may block forever. Keep its writer outside Tokio's shutdown pool,
    // with a bounded queue, so cancelling an awaiting write can still exit.
    std::thread::Builder::new()
      .name("exec-stdout".into())
      .spawn(move || {
        while let Some(packet) = receiver.blocking_recv() {
          let result = stream
            .write_all(&packet.bytes)
            .and_then(|()| stream.flush());
          let failed = result.is_err();
          let _ = packet.flushed.send(result);
          if failed {
            break;
          }
        }
      })?;
    Ok(Self { format, sender })
  }

  async fn write(&mut self, bytes: Vec<u8>) -> Result<()> {
    let (flushed, done) = oneshot::channel();
    self
      .sender
      .send(Packet { bytes, flushed })
      .await
      .context("stdout writer stopped")?;
    done.await.context("stdout writer stopped")??;
    Ok(())
  }

  pub async fn record(&mut self, mut value: Value) -> Result<()> {
    if matches!(self.format, OutputFormat::Jsonl) {
      value["schema_version"] = json!(1);
      let mut bytes = serde_json::to_vec(&value)?;
      bytes.push(b'\n');
      self.write(bytes).await?;
    }
    Ok(())
  }

  pub async fn finish(&mut self, run: &v1::Run, code: u8, reason: Option<&str>) -> Result<()> {
    self
      .record(
        json!({"type":"result", "session_id":run.session_id, "run_id":run.id,
      "status":run.status, "result":run.result, "error":run.error,
      "exit_code":code, "reason":reason}),
      )
      .await?;
    if matches!(self.format, OutputFormat::Text) && !run.result.is_empty() {
      let mut bytes = run.result.as_bytes().to_vec();
      bytes.push(b'\n');
      self.write(bytes).await?;
    }
    Ok(())
  }
}

// Pipes duplicate the OS handle rather than holding Rust's global stdout
// lock across a blocked write. Windows consoles need std's Unicode adapter.
#[cfg(unix)]
fn stdout_file() -> std::io::Result<File> {
  use std::os::fd::AsFd as _;
  Ok(File::from(std::io::stdout().as_fd().try_clone_to_owned()?))
}

#[cfg(windows)]
fn stdout_file() -> std::io::Result<Box<dyn std::io::Write + Send>> {
  use std::{io::IsTerminal as _, os::windows::io::AsHandle as _};
  if std::io::stdout().is_terminal() {
    return Ok(Box::new(std::io::stdout()));
  }
  Ok(Box::new(File::from(
    std::io::stdout().as_handle().try_clone_to_owned()?,
  )))
}

pub(super) fn diagnostic(message: &str) {
  let message = crate::view_model::safe_text(message);
  let _ = writeln!(std::io::stderr().lock(), "{message}");
}

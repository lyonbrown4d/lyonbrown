use std::{io, process::Stdio, sync::Arc};

use agent_runtime::Result;
use futures::{SinkExt, StreamExt};
use rmcp::{
  RoleClient,
  service::{RxJsonRpcMessage, TxJsonRpcMessage},
  transport::{Transport, async_rw::JsonRpcMessageCodec},
};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  process::{Child, ChildStdin, ChildStdout, Command},
  sync::Mutex,
};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::{McpLimits, StdioConfig, config::invalid};

type Reader = FramedRead<ChildStdout, JsonRpcMessageCodec<RxJsonRpcMessage<RoleClient>>>;
type Writer = FramedWrite<ChildStdin, JsonRpcMessageCodec<TxJsonRpcMessage<RoleClient>>>;

pub(crate) struct ProcessTransport {
  child: Child,
  reader: Reader,
  writer: Arc<Mutex<Option<Writer>>>,
  stderr_task: Option<tokio::task::JoinHandle<()>>,
}

pub(crate) fn start(
  config: &StdioConfig,
  id: &str,
  limits: &McpLimits,
) -> Result<ProcessTransport> {
  // Kill-on-drop also applies to failed/cancelled initialization futures.
  let mut command = Command::new(&config.executable);
  command
    .args(&config.arguments)
    .env_clear()
    .envs(&config.environment)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
  let mut child = command
    .spawn()
    .map_err(|_| invalid("cannot spawn MCP server"))?;
  let stdout = child
    .stdout
    .take()
    .ok_or_else(|| invalid("MCP stdout unavailable"))?;
  let stdin = child
    .stdin
    .take()
    .ok_or_else(|| invalid("MCP stdin unavailable"))?;
  let stderr = child
    .stderr
    .take()
    .ok_or_else(|| invalid("MCP stderr unavailable"))?;
  let log = stderr_file(config, id)?;
  // Persist a bounded private diagnostic sample, then keep draining to avoid
  // deadlocking the child. Raw stderr never enters model-facing logs or output.
  let stderr_task = tokio::spawn(async move {
    let mut stderr = stderr;
    let mut log = tokio::fs::File::from_std(log);
    let mut buffer = [0u8; 8192];
    let mut remaining = 1024 * 1024;
    loop {
      match stderr.read(&mut buffer).await {
        Ok(0) | Err(_) => break,
        Ok(size) => {
          let count = size.min(remaining);
          if count > 0 && log.write_all(&buffer[..count]).await.is_err() {
            break;
          }
          remaining -= count;
        }
      }
    }
    let _ = log.flush().await;
  });
  Ok(ProcessTransport {
    child,
    stderr_task: Some(stderr_task),
    reader: FramedRead::new(
      stdout,
      JsonRpcMessageCodec::new_with_max_length(crate::connection::wire_limit(limits)),
    ),
    writer: Arc::new(Mutex::new(Some(FramedWrite::new(
      stdin,
      JsonRpcMessageCodec::default(),
    )))),
  })
}

fn stderr_file(config: &StdioConfig, id: &str) -> Result<std::fs::File> {
  std::fs::create_dir_all(&config.stderr_directory)
    .map_err(|_| invalid("cannot create MCP diagnostic directory"))?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
      &config.stderr_directory,
      std::fs::Permissions::from_mode(0o700),
    )
    .map_err(|_| invalid("cannot protect MCP diagnostic directory"))?;
  }
  let path = config.stderr_directory.join(format!("mcp-{id}.stderr"));
  let previous = path.with_extension("stderr.previous");
  if path.exists() {
    if previous.exists() {
      std::fs::remove_file(&previous).map_err(|_| invalid("cannot rotate MCP diagnostics"))?;
    }
    std::fs::rename(&path, previous).map_err(|_| invalid("cannot rotate MCP diagnostics"))?;
  }
  let mut options = std::fs::OpenOptions::new();
  options.write(true).create_new(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
  }
  options
    .open(path)
    .map_err(|_| invalid("cannot open private MCP diagnostics"))
}

impl Transport<RoleClient> for ProcessTransport {
  type Error = io::Error;

  fn send(
    &mut self,
    message: TxJsonRpcMessage<RoleClient>,
  ) -> impl Future<Output = io::Result<()>> + Send + 'static {
    let writer = self.writer.clone();
    async move {
      writer
        .lock()
        .await
        .as_mut()
        .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?
        .send(message)
        .await
        .map_err(io::Error::other)
    }
  }

  async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
    self.reader.next().await.and_then(|message| message.ok())
  }

  async fn close(&mut self) -> io::Result<()> {
    self.writer.lock().await.take();
    self.child.kill().await?;
    self.child.wait().await?;
    if let Some(task) = self.stderr_task.take() {
      let _ = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;
    }
    Ok(())
  }
}

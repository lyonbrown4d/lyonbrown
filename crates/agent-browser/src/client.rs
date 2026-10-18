use crate::{
  BrowserError, BrowserResult, Command, Reply,
  wire::{self, Request, Response},
};
use std::{
  path::Path,
  process::Stdio,
  sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
  },
  time::Duration,
};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  process::{Child, ChildStdin, ChildStdout},
  sync::Mutex,
};

struct Transport {
  child: Child,
  input: ChildStdin,
  output: ChildStdout,
  failed: bool,
}

#[derive(Clone)]
pub struct BrowserHost {
  transport: Arc<Mutex<Transport>>,
  next_id: Arc<AtomicU64>,
}

impl BrowserHost {
  pub async fn spawn(executable: &Path, profiles: &Path) -> BrowserResult<Self> {
    let mut child = tokio::process::Command::new(executable)
      .env_clear()
      .envs(crate::restricted_environment(std::env::vars_os()))
      .arg("--browser-runtime")
      .arg("--profiles")
      .arg(profiles)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(crate::diagnostics::stderr(profiles)?)
      .kill_on_drop(true)
      .spawn()
      .map_err(|error| {
        BrowserError::Unavailable(format!("cannot start independent CEF Host: {error}"))
      })?;
    let input = child
      .stdin
      .take()
      .ok_or_else(|| BrowserError::Protocol("host stdin is missing".into()))?;
    let output = child
      .stdout
      .take()
      .ok_or_else(|| BrowserError::Protocol("host stdout is missing".into()))?;
    let host = Self {
      transport: Arc::new(Mutex::new(Transport {
        child,
        input,
        output,
        failed: false,
      })),
      next_id: Arc::new(AtomicU64::new(1)),
    };
    match host.request(Command::Hello).await? {
      Reply::Ready {
        protocol_version: crate::RUNTIME_PROTOCOL_VERSION,
        ..
      } => Ok(host),
      _ => Err(BrowserError::Protocol("unexpected host handshake".into())),
    }
  }

  /// No command is retried after a transport failure; mutation results become uncertain.
  pub async fn request(&self, command: Command) -> BrowserResult<Reply> {
    let read_only = command.read_only();
    let shutting_down = matches!(command, Command::Shutdown);
    let id = self.next_id.fetch_add(1, Ordering::Relaxed);
    let bytes = wire::encode(&Request { id, command })?;
    let mut transport = self.transport.lock().await;
    if transport.failed {
      return Err(BrowserError::Unavailable(
        "CEF Host connection failed; explicit restart is required".into(),
      ));
    }
    // Cancellation while an exchange is in flight leaves the pipe poisoned.
    // A later caller must never write behind an operation whose reply was lost.
    transport.failed = true;
    let exchange = async {
      transport.input.write_u32_le(bytes.len() as u32).await?;
      transport.input.write_all(&bytes).await?;
      transport.input.flush().await?;
      let length = transport.output.read_u32_le().await? as usize;
      if length > wire::MAX_PACKET_BYTES {
        return Err(std::io::Error::new(
          std::io::ErrorKind::InvalidData,
          "oversized host response",
        ));
      }
      let mut bytes = vec![0; length];
      transport.output.read_exact(&mut bytes).await?;
      Ok::<_, std::io::Error>(bytes)
    };
    let bytes = match tokio::time::timeout(Duration::from_secs(35), exchange).await {
      Ok(Ok(bytes)) => bytes,
      _ => {
        transport.failed = true;
        let _ = transport.child.start_kill();
        return Err(if read_only {
          BrowserError::Unavailable("CEF Host stopped responding".into())
        } else {
          BrowserError::Uncertain
        });
      }
    };
    let response: Response = match wire::decode(&bytes) {
      Ok(response) => response,
      Err(error) => {
        let _ = transport.child.start_kill();
        return Err(if read_only {
          error
        } else {
          BrowserError::Uncertain
        });
      }
    };
    if response.id != id {
      transport.failed = true;
      let _ = transport.child.start_kill();
      return Err(if read_only {
        BrowserError::Protocol("host response id mismatch".into())
      } else {
        BrowserError::Uncertain
      });
    }
    transport.failed = shutting_down;
    if shutting_down {
      match tokio::time::timeout(Duration::from_secs(15), transport.child.wait()).await {
        Ok(Ok(status)) if status.success() => {}
        _ => {
          let _ = transport.child.start_kill();
          return Err(BrowserError::Uncertain);
        }
      }
    }
    let reply = response.result?;
    match &reply {
      Reply::Frame(Some(frame)) => frame.validate()?,
      Reply::Snapshot(snapshot) => {
        if snapshot.accessibility_json.len() > crate::MAX_AX_BYTES {
          return Err(BrowserError::Protocol(
            "oversized accessibility snapshot".into(),
          ));
        }
        if let Some(frame) = &snapshot.frame {
          frame.validate()?;
        }
      }
      _ => {}
    }
    Ok(reply)
  }
}

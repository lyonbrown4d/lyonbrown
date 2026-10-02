use tokio::sync::oneshot;

const MAX_CAPTURED_STDERR_BYTES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonExit {
  exit_code: Option<i32>,
  stderr: String,
}

impl DaemonExit {
  pub fn new(exit_code: Option<i32>, stderr: impl Into<String>) -> Self {
    Self {
      exit_code,
      stderr: truncate_utf8(stderr.into(), MAX_CAPTURED_STDERR_BYTES),
    }
  }

  pub fn exit_code(&self) -> Option<i32> {
    self.exit_code
  }

  pub fn stderr(&self) -> &str {
    &self.stderr
  }

  pub(crate) fn summary(&self, limit: usize) -> String {
    let single_line = self.stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_utf8(single_line, limit)
  }
}

pub struct SpawnMonitor {
  receiver: oneshot::Receiver<DaemonExit>,
  _keepalive: Option<oneshot::Sender<DaemonExit>>,
}

impl SpawnMonitor {
  pub fn completed(exit: DaemonExit) -> Self {
    let (sender, receiver) = oneshot::channel();
    let _ = sender.send(exit);
    Self {
      receiver,
      _keepalive: None,
    }
  }

  pub fn detached() -> Self {
    let (sender, receiver) = oneshot::channel();
    Self {
      receiver,
      _keepalive: Some(sender),
    }
  }

  pub(crate) fn from_receiver(receiver: oneshot::Receiver<DaemonExit>) -> Self {
    Self {
      receiver,
      _keepalive: None,
    }
  }

  pub async fn wait(&mut self) -> DaemonExit {
    (&mut self.receiver).await.unwrap_or_else(|_| {
      DaemonExit::new(None, "daemon exit monitor stopped before reporting status")
    })
  }
}

fn truncate_utf8(mut value: String, limit: usize) -> String {
  if value.len() <= limit {
    return value;
  }
  let mut boundary = limit;
  while !value.is_char_boundary(boundary) {
    boundary -= 1;
  }
  value.truncate(boundary);
  value
}

use crate::{Plan, build::product_command};
use anyhow::{Context, Result, ensure};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio::{
  io::AsyncReadExt,
  process::{ChildStdin, ChildStdout},
};

pub(crate) struct OwnedProcess {
  child: Box<dyn ChildWrapper>,
  stopped: bool,
  parent_lease: Option<ChildStdin>,
  ready_output: Option<ChildStdout>,
  ready_line: Vec<u8>,
  incarnation: String,
}
impl OwnedProcess {
  pub fn spawn(
    binary: &Path,
    root: &Path,
    plan: &Plan,
    ambient: &BTreeMap<String, String>,
  ) -> Result<Self> {
    ensure!(
      cfg!(any(unix, windows)),
      "development process supervision is unavailable on this platform"
    );
    let command = product_command(binary, root, plan, ambient);
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let mut wrapped = CommandWrap::with_new(binary, |child| {
      child
        .current_dir(root)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
      for (key, value) in command.get_envs() {
        if let Some(value) = value {
          child.env(key, value);
        } else {
          child.env_remove(key);
        }
      }
      child.env("LYONBROWN_DEV_READY_NONCE", &nonce);
    });
    wrapped.wrap(KillOnDrop);
    #[cfg(unix)]
    wrapped.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    wrapped.wrap(process_wrap::tokio::JobObject);
    let mut child = wrapped.spawn().context("start development agentd")?;
    let parent_lease = child
      .stdin()
      .take()
      .context("capture owned daemon parent lifetime lease")?;
    let ready_output = child
      .stdout()
      .take()
      .context("capture owned daemon readiness")?;
    Ok(Self {
      child,
      stopped: false,
      parent_lease: Some(parent_lease),
      ready_output: Some(ready_output),
      ready_line: format!("lyonbrown-dev-ready:{nonce}\n").into_bytes(),
      incarnation: nonce,
    })
  }
  pub fn incarnation(&self) -> &str {
    &self.incarnation
  }
  pub async fn wait_ready(&mut self) -> Result<()> {
    let mut stdout = self
      .ready_output
      .take()
      .context("owned readiness already consumed")?;
    let mut received = Vec::new();
    let mut bytes = [0u8; 128];
    loop {
      let count = stdout.read(&mut bytes).await?;
      ensure!(
        count > 0,
        "owned development agentd exited before readiness; inspect development logs"
      );
      received.extend_from_slice(&bytes[..count]);
      ensure!(
        self.ready_line.starts_with(&received),
        "invalid owned development readiness output"
      );
      if received == self.ready_line {
        return Ok(());
      }
    }
  }
  pub fn ensure_running(&mut self) -> Result<()> {
    ensure!(
      self.child.try_wait()?.is_none(),
      "owned development agentd exited; inspect development data/logs"
    );
    Ok(())
  }
  pub async fn wait(&mut self) -> Result<()> {
    let status = self.child.wait().await?;
    anyhow::bail!("owned development agentd exited ({status}); inspect development data/logs")
  }
  pub async fn stop(&mut self) -> Result<()> {
    if self.stopped {
      return Ok(());
    }
    // Closing this private pipe requests graceful shutdown on every supported platform.
    drop(self.parent_lease.take());
    if self.child.try_wait()?.is_some() {
      self.stopped = true;
      return Ok(());
    }
    #[cfg(unix)]
    {
      // POSIX SIGTERM is handled by agentd's graceful shutdown path.
      if self.child.try_wait()?.is_none() {
        let _ = self.child.signal(15);
      }
    }
    if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(20), self.child.wait()).await {
      self.stopped = true;
      return Ok(());
    }
    self.child.start_kill()?;
    tokio::time::timeout(Duration::from_secs(5), self.child.wait())
      .await
      .context("wait for owned daemon termination")??;
    self.stopped = true;
    Ok(())
  }
}
impl Drop for OwnedProcess {
  fn drop(&mut self) {
    drop(self.parent_lease.take());
    if !self.stopped {
      let _ = self.child.start_kill();
    }
  }
}

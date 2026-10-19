use anyhow::{Context, Result, ensure};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{
  process::{Command, Output, Stdio},
  time::Duration,
};
use tokio::io::{AsyncBufReadExt, BufReader};

fn spawn(command: Command) -> Result<Box<dyn ChildWrapper>> {
  let mut wrapped = CommandWrap::from(tokio::process::Command::from(command));
  wrapped.wrap(KillOnDrop);
  #[cfg(unix)]
  wrapped.wrap(process_wrap::tokio::ProcessGroup::leader());
  #[cfg(windows)]
  wrapped.wrap(process_wrap::tokio::JobObject);
  wrapped
    .spawn()
    .context("start owned Gateway development process")
}
pub(super) async fn output(mut command: Command) -> Result<Output> {
  command.stdout(Stdio::piped()).stderr(Stdio::piped());
  let child = spawn(command)?;
  let result = tokio::select! {
    _ = tokio::signal::ctrl_c() => anyhow::bail!("Gateway tool preparation cancelled"),
    result = Box::into_pin(child.wait_with_output()) => result?,
  };
  ensure!(
    result.status.success(),
    "Gateway preparation tool failed; check its installation and toolchain"
  );
  Ok(result)
}
pub(super) async fn status(mut command: Command) -> Result<()> {
  command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
  let mut child = spawn(command)?;
  let status = tokio::select! {
    _ = tokio::signal::ctrl_c() => { Box::into_pin(child.kill()).await?; anyhow::bail!("Gateway build cancelled"); },
    status = child.wait() => status?,
  };
  ensure!(status.success(), "Gateway build tool failed");
  Ok(())
}
pub(super) async fn executable(mut command: Command) -> Result<std::path::PathBuf> {
  command.stdout(Stdio::piped()).stderr(Stdio::inherit());
  let mut child = spawn(command)?;
  let stdout = child
    .stdout()
    .take()
    .context("capture Cargo artifact messages")?;
  let read = async {
    let mut lines = BufReader::new(stdout).lines();
    let mut executable = None;
    while let Some(line) = lines.next_line().await? {
      let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
        continue;
      };
      if value["reason"] == "compiler-message"
        && let Some(rendered) = value["message"]["rendered"].as_str()
      {
        eprint!("{rendered}");
      } else if value["reason"] == "compiler-artifact"
        && value["target"]["name"] == "ai-gateway"
        && let Some(path) = value["executable"].as_str()
      {
        executable = Some(std::path::PathBuf::from(path));
      }
    }
    executable.context("Cargo did not report the ai-gateway executable")
  };
  let executable = tokio::select! {
    _ = tokio::signal::ctrl_c() => { Box::into_pin(child.kill()).await?; anyhow::bail!("Gateway build cancelled"); },
    result = read => result?,
  };
  ensure!(child.wait().await?.success(), "Gateway server build failed");
  Ok(executable)
}
pub(super) async fn serve(command: Command) -> Result<()> {
  let mut child = spawn(command)?;
  tracing::info!(
    event = "gateway_dev_started",
    "Gateway development server started"
  );
  tokio::select! {
    result = child.wait() => {
      ensure!(result?.success(), "Gateway server exited unsuccessfully; inspect Gateway logs");
    },
    _ = tokio::signal::ctrl_c() => {
      #[cfg(unix)]
      { let _ = child.signal(2); }
      #[cfg(windows)]
      { child.start_kill()?; }
      match tokio::time::timeout(Duration::from_secs(20), child.wait()).await {
        Ok(result) => { result?; },
        Err(_) => { Box::into_pin(child.kill()).await?; },
      }
    },
  }
  tracing::info!(
    event = "gateway_dev_stopped",
    "Owned Gateway development server stopped"
  );
  Ok(())
}

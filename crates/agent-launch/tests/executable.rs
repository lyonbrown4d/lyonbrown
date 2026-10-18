#![cfg(target_os = "macos")]

use anyhow::{Context, Result, ensure};
use std::{
  fs,
  io::{self, Write},
  os::unix::fs::{PermissionsExt, symlink},
  path::{Path, PathBuf},
  process::Stdio,
  time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

const PROBE: &str = "LYONBROWN_TEST_LOADED_EXECUTABLE";
const READY: &str = "loaded-executable-ready:";
const RESULT: &str = "loaded-executable-result:";

#[test]
fn native_process_probe_child() -> Result<()> {
  if std::env::var_os(PROBE).is_none() {
    return Ok(());
  }
  println!("\n{READY}");
  io::stdout().flush()?;
  let mut line = String::new();
  io::stdin().read_line(&mut line)?;
  ensure!(line == "resolve\n", "probe handshake was not authorized");
  let executable = agent_launch::current_executable()?;
  let options = agent_launch::LaunchOptions::bundled(std::iter::empty::<&str>())?;
  println!(
    "{RESULT}{}",
    serde_json::json!({"executable": executable, "daemon": options.bundled_agentd})
  );
  Ok(())
}

async fn marker(
  lines: &mut Lines<BufReader<tokio::process::ChildStdout>>,
  prefix: &str,
) -> Result<String> {
  loop {
    let line = lines
      .next_line()
      .await?
      .context("probe exited before handshake")?;
    if let Some(value) = line.strip_prefix(prefix) {
      return Ok(value.to_owned());
    }
  }
}

fn app(root: &Path, version: &str, client: &str) -> Result<PathBuf> {
  let app = root.join(version).join("Lyonbrown.app");
  let binary = app.join("Contents/MacOS").join(client);
  fs::create_dir_all(binary.parent().context("fixture executable parent")?)?;
  fs::copy(std::env::current_exe()?, &binary)?;
  let daemon = binary.with_file_name("agentd");
  fs::write(&daemon, b"isolated sibling marker; never executed")?;
  fs::set_permissions(&daemon, fs::Permissions::from_mode(0o755))?;
  Ok(app)
}

#[tokio::test]
async fn selector_upgrade_cannot_rebind_a_running_client_or_its_bundled_daemon() -> Result<()> {
  for desktop in [false, true] {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    let name = if desktop {
      "agent-desktop"
    } else {
      "agent-cli"
    };
    let old = app(root, "old", name)?;
    let new = app(root, "new", name)?;
    let public = root.join(if desktop { "Lyonbrown.app" } else { "agent" });
    let old_target = if desktop {
      old.clone()
    } else {
      old.join("Contents/MacOS").join(name)
    };
    let new_target = if desktop {
      new
    } else {
      new.join("Contents/MacOS").join(name)
    };
    symlink(old_target, &public)?;
    let entry = if desktop {
      public.join("Contents/MacOS").join(name)
    } else {
      public.clone()
    };
    let mut child = tokio::process::Command::new(entry)
      .args(["--exact", "native_process_probe_child", "--nocapture"])
      .env(PROBE, "1")
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .kill_on_drop(true)
      .spawn()?;
    let mut stdout = BufReader::new(child.stdout.take().context("probe stdout")?).lines();
    tokio::time::timeout(Duration::from_secs(10), marker(&mut stdout, READY)).await??;
    let pending = root.join("next-selector");
    symlink(new_target, &pending)?;
    fs::rename(pending, &public)?;
    let mut stdin = child.stdin.take().context("probe stdin")?;
    stdin.write_all(b"resolve\n").await?;
    drop(stdin);
    let result =
      tokio::time::timeout(Duration::from_secs(10), marker(&mut stdout, RESULT)).await??;
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait()).await??;
    ensure!(status.success(), "native process probe failed: {status}");
    let actual: serde_json::Value = serde_json::from_str(&result)?;
    let executable = old.join("Contents/MacOS").join(name).canonicalize()?;
    ensure!(
      actual["executable"] == serde_json::json!(executable),
      "running client rebound to changed selector: {actual}"
    );
    ensure!(
      actual["daemon"] == serde_json::json!(executable.with_file_name("agentd")),
      "bundled daemon rebound to changed selector: {actual}"
    );
  }
  Ok(())
}

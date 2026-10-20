/// Drive the product's actual Config::load path in a child process, without
/// mutating this test process's environment. The configured child records its
/// environment and exits without speaking MCP, so the daemon must reject it.
#[cfg(unix)]
pub async fn verify_child_environment_confinement() {
  use anyhow::Context;
  use std::{process::Stdio, time::Duration};

  let data = tempfile::tempdir().expect("daemon data");
  let captured = data.path().join("child-environment.txt");
  let stderr = tempfile::NamedTempFile::new_in(data.path()).expect("private daemon stderr");
  let path_argument =
    serde_json::to_string(captured.to_str().expect("UTF-8 path")).expect("argument encoding");
  // Keep the shell's MCP stdout open until env finishes writing the capture.
  // An exec redirection would close it early and let MCP teardown kill env.
  let config = format!(
    r#"
[[mcp]]
id = "fixture"
allowed_tools = ["echo"]
[mcp.transport]
type = "stdio"
executable = "/bin/sh"
arguments = ["-c", "/usr/bin/env > \"$1\"", "fixture", {path_argument}]
environment = {{ CHILD_TOKEN = "MCP_TOKEN" }}
"#
  );
  let config_path = data.path().join("config.toml");
  std::fs::write(&config_path, config).expect("product config");
  std::fs::write(
    data.path().join(".env"),
    "MCP_TOKEN=synthetic-dotenv-child\n",
  )
  .expect("dotenv");
  let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_agentd"));
  command
    .args([
      "--data-dir",
      data.path().to_str().expect("data UTF-8"),
      "--config-file",
      config_path.to_str().expect("config UTF-8"),
    ])
    .env_clear()
    .env("OPENAI_API_KEY", "synthetic-ambient-provider")
    .env("MCP_TOKEN", "synthetic-explicit-child")
    .stdout(Stdio::null())
    .stderr(stderr.as_file().try_clone().expect("capture daemon stderr"))
    .kill_on_drop(true);
  let mut child = command.spawn().expect("start product binary");
  let outcome: anyhow::Result<_> = async {
    // Loading the actual product binary is a cold OS operation, separate from
    // the MCP handshake deadline that starts once its configured child runs.
    tokio::time::timeout(Duration::from_secs(60), async {
      loop {
        if captured
          .try_exists()
          .context("check MCP child environment capture")?
        {
          return Ok::<(), anyhow::Error>(());
        }
        if let Some(status) = child.try_wait().context("inspect daemon startup")? {
          anyhow::bail!("daemon exited before MCP child environment capture ({status})");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    })
    .await
    .context("cold daemon startup did not reach the configured MCP child")??;
    // Allow the full public handshake budget plus bounded process exit cleanup.
    let rejection_budget =
      agent_mcp::McpLimits::default().connection_timeout + Duration::from_secs(5);
    tokio::time::timeout(rejection_budget, child.wait())
      .await
      .context("daemon did not exit after rejecting the non-MCP child")?
      .context("wait for daemon rejection")
  }
  .await;
  let status = match outcome {
    Ok(status) => status,
    Err(error) => {
      let kill = child.start_kill();
      let reaped = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
      panic!("{error:#}; fixture cleanup: kill={kill:?}, reap={reaped:?}");
    }
  };
  assert!(
    !status.success(),
    "non-MCP child must not be accepted as a supported server"
  );
  let environment = std::fs::read_to_string(captured).expect("captured child environment");
  let child_token = environment
    .lines()
    .find_map(|line| line.strip_prefix("CHILD_TOKEN="));
  let source = match child_token {
    Some("synthetic-explicit-child") => "explicit environment",
    Some("synthetic-dotenv-child") => "dotenv",
    Some(_) => "unexpected value",
    None => "missing",
  };
  assert!(
    child_token == Some("synthetic-explicit-child"),
    "MCP child token source: {source}; captured environment: {} bytes",
    environment.len()
  );
  assert!(!environment.contains("OPENAI_API_KEY"));
  assert!(!environment.contains("synthetic-ambient-provider"));
  assert!(!environment.contains("MCP_TOKEN="));
  let stderr = std::fs::read(stderr.path()).expect("read private daemon stderr");
  assert!(!String::from_utf8_lossy(&stderr).contains("synthetic-explicit-child"));
}

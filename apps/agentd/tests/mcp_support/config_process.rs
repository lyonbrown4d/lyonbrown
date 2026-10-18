/// Drive the product's actual Config::load path in a child process, without
/// mutating this test process's environment. The child exits before MCP
/// initialization, after recording exactly which environment it received.
#[cfg(unix)]
pub async fn verify_child_environment_confinement() {
  let data = tempfile::tempdir().expect("daemon data");
  let captured = data.path().join("child-environment.txt");
  let path_argument =
    serde_json::to_string(captured.to_str().expect("UTF-8 path")).expect("argument encoding");
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
    .kill_on_drop(true);
  let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
    .await
    .expect("bounded daemon startup")
    .expect("start product binary");
  assert!(
    !output.status.success(),
    "non-MCP child must not be accepted as a supported server"
  );
  let environment = std::fs::read_to_string(captured).expect("captured child environment");
  assert!(
    environment
      .lines()
      .any(|line| line == "CHILD_TOKEN=synthetic-explicit-child")
  );
  assert!(!environment.contains("OPENAI_API_KEY"));
  assert!(!environment.contains("synthetic-ambient-provider"));
  assert!(!environment.contains("MCP_TOKEN="));
  assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-explicit-child"));
}

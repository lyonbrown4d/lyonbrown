use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use agent_cli::cli::{Cli, daemon_fixed_arguments, sibling_agentd_path};
use clap::Parser;

#[test]
fn daemon_path_is_sibling_of_agent_binary() {
  let agent = Path::new("/opt/lyonbrown/bin/agent");
  let expected = if cfg!(windows) {
    PathBuf::from("/opt/lyonbrown/bin/agentd.exe")
  } else {
    PathBuf::from("/opt/lyonbrown/bin/agentd")
  };

  assert_eq!(sibling_agentd_path(agent).unwrap(), expected);
}

#[test]
fn daemon_arguments_preserve_runtime_overrides_without_secrets() {
  let cli = Cli::try_parse_from([
    "agent",
    "--config",
    "agent.toml",
    "--provider",
    "openai",
    "--model",
    "gpt-test",
    "--endpoint",
    "http://127.0.0.1:8080",
    "--deny-all",
    "--trust-workspace-mcp",
    "--trust-workspace-config",
  ])
  .unwrap();

  let arguments = daemon_fixed_arguments(&cli);
  for expected in [
    "--config",
    "agent.toml",
    "--provider",
    "openai",
    "--model",
    "gpt-test",
    "--endpoint",
    "http://127.0.0.1:8080",
    "--deny-all",
    "--trust-workspace-mcp",
    "--trust-workspace-config",
  ] {
    assert!(arguments.iter().any(|value| value == OsStr::new(expected)));
  }
  assert!(!arguments.iter().any(|value| {
    value
      .to_string_lossy()
      .to_ascii_lowercase()
      .contains("token")
  }));
}

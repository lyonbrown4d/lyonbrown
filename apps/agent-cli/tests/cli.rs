use std::path::Path;

use agent_cli::cli::Cli;
use clap::Parser;

#[test]
fn installed_binary_is_named_agent() {
  let binary = Path::new(env!("CARGO_BIN_EXE_agent"));

  assert_eq!(binary.file_stem().unwrap(), "agent");
  let output = std::process::Command::new(binary)
    .arg("--help")
    .output()
    .unwrap();
  assert!(output.status.success());
  let help = String::from_utf8_lossy(&output.stdout);
  assert!(help.contains("[OPTIONS] [WORKSPACE]"));
  assert!(!help.contains("Commands:"));
}

#[test]
fn no_arguments_open_the_current_workspace() {
  let cli = Cli::try_parse_from(["agent"]).unwrap();

  assert_eq!(cli.workspace, Path::new("."));
}

#[test]
fn workspace_and_daemon_overrides_are_flat_options() {
  let cli = Cli::try_parse_from([
    "agent",
    "project",
    "--provider",
    "ollama",
    "--model",
    "qwen3:8b",
    "--endpoint",
    "http://127.0.0.1:11434",
    "--approve-all",
    "--trust-workspace-mcp",
    "--trust-workspace-config",
  ])
  .unwrap();

  assert_eq!(cli.workspace, Path::new("project"));
  assert_eq!(cli.provider.as_deref(), Some("ollama"));
  assert_eq!(cli.model.as_deref(), Some("qwen3:8b"));
  assert_eq!(cli.endpoint.as_deref(), Some("http://127.0.0.1:11434"));
  assert!(cli.approve_all);
  assert!(cli.trust_workspace_mcp);
  assert!(cli.trust_workspace_config);
}

#[test]
fn embedded_subcommands_are_not_recognized_as_commands() {
  let cli = Cli::try_parse_from(["agent", "chat"]).unwrap();
  assert_eq!(cli.workspace, Path::new("chat"));

  assert!(Cli::try_parse_from(["agent", "ask", "hello"]).is_err());
  assert!(Cli::try_parse_from(["agent", "sessions", "extra"]).is_err());
}

#[test]
fn approve_all_and_deny_all_are_mutually_exclusive() {
  assert!(Cli::try_parse_from(["agent", "--approve-all", "--deny-all"]).is_err());
  let cli = Cli::try_parse_from(["agent", "--deny-all"]).unwrap();
  assert!(cli.deny_all);
}

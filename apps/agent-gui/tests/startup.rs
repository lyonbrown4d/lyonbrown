use std::ffi::OsString;
use std::path::Path;

use agent_gui::startup::{GuiArgs, daemon_fixed_arguments, sanitize_connection_error};
use clap::Parser;

#[test]
fn gui_arguments_propagate_runtime_overrides_to_agentd() {
  let args = GuiArgs::try_parse_from([
    "agent-gui",
    "workspace",
    "--config",
    "agent.yaml",
    "--provider",
    "ollama",
    "--model",
    "qwen3",
    "--profile",
    "reviewer",
    "--endpoint",
    "http://127.0.0.1:11434",
    "--approve-all",
    "--trust-workspace-mcp",
    "--trust-workspace-config",
  ])
  .unwrap();

  assert_eq!(args.workspace, Path::new("workspace"));
  assert_eq!(
    daemon_fixed_arguments(&args),
    vec![
      OsString::from("--config"),
      OsString::from("agent.yaml"),
      OsString::from("--provider"),
      OsString::from("ollama"),
      OsString::from("--model"),
      OsString::from("qwen3"),
      OsString::from("--profile"),
      OsString::from("reviewer"),
      OsString::from("--endpoint"),
      OsString::from("http://127.0.0.1:11434"),
      OsString::from("--approve-all"),
      OsString::from("--trust-workspace-mcp"),
      OsString::from("--trust-workspace-config"),
    ]
  );
}

#[test]
fn mutually_exclusive_approval_overrides_are_rejected() {
  let result = GuiArgs::try_parse_from(["agent-gui", "--approve-all", "--deny-all"]);

  assert!(result.is_err());
}

#[test]
fn connection_errors_are_single_line_and_bounded_for_the_banner() {
  let untrusted = format!("launch failed\r\nsecret-ish detail {}", "x".repeat(700));

  let sanitized = sanitize_connection_error(&untrusted);

  assert!(!sanitized.contains(['\r', '\n']));
  assert!(sanitized.chars().count() <= 512);
  assert!(sanitized.starts_with("launch failed  secret-ish detail"));
}

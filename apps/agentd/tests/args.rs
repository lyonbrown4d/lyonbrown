use std::path::PathBuf;

use agentd::args::Args;
use clap::{CommandFactory as _, Parser as _};

#[test]
fn parses_workspace_config_and_trust_boundaries() {
  let args = Args::try_parse_from([
    "agentd",
    "--foreground",
    "--workspace",
    "project",
    "--config",
    "daemon.toml",
    "--runtime-config-fingerprint",
    "runtime-v1",
    "--provider",
    "ollama",
    "--profile",
    "default-dev",
    "--model",
    "qwen3:8b",
    "--endpoint",
    "http://127.0.0.1:11434",
    "--approve-all",
    "--trust-workspace-mcp",
    "--trust-workspace-config",
  ])
  .expect("valid daemon arguments");

  assert!(args.foreground);
  assert_eq!(args.workspace, PathBuf::from("project"));
  assert_eq!(args.config, Some(PathBuf::from("daemon.toml")));
  assert_eq!(args.runtime_config_fingerprint, "runtime-v1");
  assert_eq!(args.provider.as_deref(), Some("ollama"));
  assert_eq!(args.profile.as_deref(), Some("default-dev"));
  assert_eq!(args.model.as_deref(), Some("qwen3:8b"));
  assert_eq!(args.endpoint.as_deref(), Some("http://127.0.0.1:11434"));
  assert!(args.approve_all);
  assert!(!args.deny_all);
  assert!(args.trust_workspace_mcp);
  assert!(args.trust_workspace_config);
}

#[test]
fn approve_all_and_deny_all_conflict() {
  let error = Args::try_parse_from([
    "agentd",
    "--runtime-config-fingerprint",
    "runtime-v1",
    "--approve-all",
    "--deny-all",
  ])
  .expect_err("approval modes must conflict");

  assert!(error.to_string().contains("cannot be used with"));
}

#[test]
fn daemon_cli_does_not_offer_a_public_bind_argument() {
  let command = Args::command();

  assert!(
    command
      .get_arguments()
      .all(|argument| argument.get_id() != "bind")
  );
  assert!(
    Args::try_parse_from([
      "agentd",
      "--workspace",
      ".",
      "--runtime-config-fingerprint",
      "runtime-v1",
      "--bind",
      "0.0.0.0:9999",
    ])
    .is_err()
  );
}

#[test]
fn runtime_fingerprint_must_not_be_blank() {
  let error = Args::try_parse_from([
    "agentd",
    "--workspace",
    ".",
    "--runtime-config-fingerprint",
    "   ",
  ])
  .expect_err("blank fingerprint must be rejected");

  assert!(error.to_string().contains("must not be blank"));
}

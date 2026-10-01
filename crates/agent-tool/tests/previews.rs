use std::sync::Arc;
use std::time::Duration;

use agent_core::{SandboxStatus, SecretRef, ToolCallId, ToolInvocation};
use agent_secret::NoopBroker;
use agent_tool::{ShellTool, Tool, ToolContext, WriteFileTool};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn context() -> ToolContext {
  ToolContext::new(
    std::path::PathBuf::from("C:/workspace/project"),
    Arc::new(NoopBroker),
    CancellationToken::new(),
    Duration::from_secs(1),
  )
}

#[test]
fn shell_preview_is_deterministic_and_explicitly_unsandboxed() {
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "shell".into(),
    arguments: json!({"command": "cargo test --workspace"}),
  };

  let first = ShellTool::new().preview(&context(), &invocation).unwrap();
  let second = ShellTool::new().preview(&context(), &invocation).unwrap();

  assert_eq!(first, second);
  assert_eq!(first.target, "cargo test --workspace");
  assert_eq!(first.sandbox, SandboxStatus::HostUnsandboxed);
  assert_eq!(
    first.working_directory,
    Some(std::path::PathBuf::from("C:/workspace/project"))
  );
  assert!(first.impact.contains("host"));
  assert!(first.impact.contains("unsandboxed"));
}

#[test]
fn shell_preview_contains_secret_names_but_never_secret_values() {
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "shell".into(),
    arguments: json!({
      "command": "deploy",
      "env": {
        "TOKEN": "secret:env:DEPLOY_TOKEN",
        "CONFIG": "secret:file:production"
      }
    }),
  };

  let preview = ShellTool::new().preview(&context(), &invocation).unwrap();
  let serialized = serde_json::to_string(&preview).unwrap();

  assert_eq!(
    preview.required_secrets,
    vec![
      SecretRef::env("DEPLOY_TOKEN"),
      SecretRef::file("production")
    ]
  );
  assert!(!serialized.contains("actual-secret-value"));
  assert!(!serialized.contains("TOKEN\":"));
}

#[test]
fn write_file_preview_describes_workspace_target_without_content() {
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "write_file".into(),
    arguments: json!({"path": "src/main.rs", "content": "private body"}),
  };

  let preview = WriteFileTool::new()
    .preview(&context(), &invocation)
    .unwrap();
  let serialized = serde_json::to_string(&preview).unwrap();

  assert_eq!(preview.target, "src/main.rs");
  assert_eq!(preview.sandbox, SandboxStatus::WorkspaceRestricted);
  assert!(!serialized.contains("private body"));
}

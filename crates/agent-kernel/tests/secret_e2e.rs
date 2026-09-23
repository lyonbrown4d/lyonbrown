use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_core::AgentEvent;
use agent_kernel::Kernel;
use agent_model::{ModelRegistry, Role};
use agent_secret::{CompositeBroker, FileSecretBroker, SecretBroker};
use agent_tool::{AllowAllApproval, ShellTool, ToolRegistry};
use provider_mock::{MockProvider, ScriptedStep};
use serde_json::json;

const SECRET: &str = "m5-private-value-9f36";

struct TestDir(PathBuf);

impl TestDir {
  fn new(label: &str) -> Self {
    let path = std::env::temp_dir().join(format!(
      "lyonbrown-m5-{label}-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    Self(path)
  }

  fn path(&self) -> &Path {
    &self.0
  }
}

impl Drop for TestDir {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

fn shell_echo(variable: &str) -> String {
  if cfg!(windows) {
    format!("echo %{variable}%")
  } else {
    format!("printf %s \"${variable}\"")
  }
}

fn kernel(workspace: &Path, secret_root: &Path, script: Vec<ScriptedStep>) -> Kernel {
  let provider = Arc::new(MockProvider::scripted(script));
  let registry = ModelRegistry::new();
  registry.register(provider);
  let tools = ToolRegistry::new();
  tools.register(Arc::new(ShellTool::new()));
  let file_broker = FileSecretBroker::new(secret_root).unwrap();
  let broker = CompositeBroker::new(vec![Arc::new(file_broker) as Arc<dyn SecretBroker>]);

  Kernel::builder()
    .providers(registry)
    .tools(tools)
    .approval(AllowAllApproval)
    .broker(broker)
    .workspace_root(workspace)
    .build()
}

#[tokio::test]
async fn shell_secret_is_late_bound_and_never_reaches_events_or_model_context() {
  let workspace = TestDir::new("workspace");
  let secret_root = TestDir::new("secrets");
  FileSecretBroker::new(secret_root.path())
    .unwrap()
    .store("test_value", SECRET)
    .unwrap();
  let mut kernel = kernel(
    workspace.path(),
    secret_root.path(),
    vec![
      ScriptedStep::tool_call(
        "shell",
        json!({
            "command": shell_echo("M5_TOKEN"),
            "env": {"M5_TOKEN": "secret:file:test_value"}
        }),
      ),
      ScriptedStep::text("done"),
    ],
  );

  let events = kernel.run_turn("read the protected value").await.unwrap();

  let result = events.iter().find_map(AgentEvent::tool_result).unwrap();
  assert!(result.ok);
  assert!(result.output.contains("[REDACTED]"));
  assert!(!result.output.contains(SECRET));
  assert!(!serde_json::to_string(&events).unwrap().contains(SECRET));
  assert!(kernel.last_requests().iter().all(|request| {
    request
      .system
      .as_deref()
      .is_none_or(|system| !system.contains(SECRET))
      && request
        .messages
        .iter()
        .all(|message| !message.content.contains(SECRET))
  }));
  assert!(
    kernel.last_requests()[1]
      .messages
      .iter()
      .any(|message| message.role == Role::Tool && message.content.contains("[REDACTED]"))
  );
}

#[tokio::test]
async fn missing_secret_becomes_a_failed_tool_result_without_panicking_or_leaking() {
  let workspace = TestDir::new("workspace-missing");
  let secret_root = TestDir::new("secrets-missing");
  let mut kernel = kernel(
    workspace.path(),
    secret_root.path(),
    vec![
      ScriptedStep::tool_call(
        "shell",
        json!({
            "command": shell_echo("M5_TOKEN"),
            "env": {"M5_TOKEN": "secret:file:missing"}
        }),
      ),
      ScriptedStep::text("recovered"),
    ],
  );

  let events = kernel.run_turn("use a missing secret").await.unwrap();

  let result = events.iter().find_map(AgentEvent::tool_result).unwrap();
  assert!(!result.ok);
  assert!(result.output.is_empty());
  assert!(
    result
      .error
      .as_deref()
      .unwrap()
      .contains("secret not found")
  );
  assert_eq!(events.last().unwrap().kind(), "TurnCompleted");
}

use std::sync::Arc;
use std::time::Duration;

use agent_core::{RiskLevel, ToolCallId, ToolDefinition, ToolInvocation, ToolResult, ToolSource};
use agent_secret::NoopBroker;
use agent_tool::{
  AllowAllApproval, ApprovalDecision, ApprovalPolicy, EchoTool, ReadFileTool, Tool, ToolContext,
  ToolError, ToolExecutionError, ToolExecutor, ToolRegistry,
};
use async_trait::async_trait;
use serde_json::json;
use tokio_util::sync::CancellationToken;

struct PendingTool {
  definition: ToolDefinition,
}

impl PendingTool {
  fn new() -> Self {
    Self {
      definition: ToolDefinition {
        name: "pending".into(),
        description: "never completes".into(),
        input_schema: json!({"type": "object"}),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Read,
        requires_secrets: vec![],
      },
    }
  }
}

#[async_trait]
impl Tool for PendingTool {
  fn definition(&self) -> &ToolDefinition {
    &self.definition
  }

  async fn invoke(
    &self,
    _ctx: &ToolContext,
    _invocation: &ToolInvocation,
  ) -> Result<ToolResult, ToolError> {
    std::future::pending().await
  }
}

fn context(root: std::path::PathBuf, timeout: Duration) -> ToolContext {
  ToolContext::new(
    root,
    Arc::new(NoopBroker),
    CancellationToken::new(),
    timeout,
  )
}

#[tokio::test]
async fn echo_tool_returns_the_text_argument() {
  let registry = ToolRegistry::new();
  registry.register(Arc::new(EchoTool::new()));
  let executor = ToolExecutor::new(registry, context(".".into(), Duration::from_secs(1)));
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "echo".into(),
    arguments: json!({"text": "hello"}),
  };

  let result = executor.execute(&invocation).await.unwrap();

  assert!(result.ok);
  assert_eq!(result.output, "hello");
}

#[tokio::test]
async fn executor_times_out_a_hung_tool() {
  let registry = ToolRegistry::new();
  registry.register(Arc::new(PendingTool::new()));
  let executor = ToolExecutor::new(registry, context(".".into(), Duration::from_millis(10)));
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "pending".into(),
    arguments: json!({}),
  };

  let error = executor.execute(&invocation).await.unwrap_err();

  assert!(matches!(error, ToolExecutionError::TimedOut { .. }));
}

#[tokio::test]
async fn read_file_stays_inside_the_workspace_root() {
  let test_root =
    std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(ToolCallId::new().to_string());
  std::fs::create_dir_all(&test_root).unwrap();
  std::fs::write(test_root.join("note.txt"), "inside").unwrap();

  let registry = ToolRegistry::new();
  registry.register(Arc::new(ReadFileTool::new()));
  let executor = ToolExecutor::new(registry, context(test_root.clone(), Duration::from_secs(1)));
  let allowed = ToolInvocation {
    id: ToolCallId::new(),
    name: "read_file".into(),
    arguments: json!({"path": "note.txt"}),
  };
  let traversal = ToolInvocation {
    id: ToolCallId::new(),
    name: "read_file".into(),
    arguments: json!({"path": "../outside.txt"}),
  };

  assert_eq!(executor.execute(&allowed).await.unwrap().output, "inside");
  assert!(matches!(
    executor.execute(&traversal).await,
    Err(ToolExecutionError::Tool(ToolError::InvalidArguments(_)))
  ));
}

#[test]
fn allow_all_policy_approves_every_risk_level() {
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "echo".into(),
    arguments: json!({"text": "hello"}),
  };
  let definition = EchoTool::new().definition().clone();

  assert_eq!(
    AllowAllApproval.approve(&definition, &invocation),
    ApprovalDecision::Allow
  );
}

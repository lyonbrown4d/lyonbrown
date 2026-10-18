#![cfg(feature = "test-fixtures")]
use agent_mcp::{
  McpApproval, McpAuthorization, McpLimits, McpManager, McpServerConfig, McpTransport, StdioConfig,
};
use agent_runtime::{RuntimeTool, ToolCall};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn sdk_stdio_fixture_handshakes_calls_and_closes_with_private_stderr() {
  let logs = tempfile::tempdir().expect("private logs");
  let config = McpServerConfig {
    id: "fixture".into(),
    transport: McpTransport::Stdio(StdioConfig {
      executable: env!("CARGO_BIN_EXE_agent-mcp-fixture").into(),
      arguments: vec![],
      environment: BTreeMap::new(),
      stderr_directory: logs.path().to_owned(),
    }),
    allowed_tools: ["echo".into()].into_iter().collect(),
  };
  let approval = Arc::new(Allow);
  let manager = McpManager::connect(vec![config], approval, McpLimits::default())
    .await
    .expect("stdio handshake");
  let result = manager.execute(ToolCall { id: "stdio-intent".into(), arguments: json!({"call":{"server": "fixture", "tool": "echo", "arguments": {"message": "over pipes"}}}) }, CancellationToken::new()).await.expect("stdio call");
  assert!(result.text.contains("over pipes"));
  assert!(!result.text.contains("fixture stderr"));
  manager.shutdown().await;
  let path = logs.path().join("mcp-fixture.stderr");
  let contents = std::fs::read_to_string(&path).expect("persisted stderr");
  assert!(contents.contains("fixture stderr is private"));
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
      std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777,
      0o600
    );
  }
}

struct Allow;
#[async_trait::async_trait]
impl McpAuthorization for Allow {
  async fn approve(&self, _: McpApproval, _: CancellationToken) -> agent_runtime::Result<()> {
    Ok(())
  }
}

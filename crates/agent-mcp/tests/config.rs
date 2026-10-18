use agent_mcp::{
  McpApproval, McpAuthorization, McpLimits, McpManager, McpServerConfig, McpTransport,
  SecretHeaders,
};
use agent_runtime::{Result, RuntimeError};
use async_trait::async_trait;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

struct Reject;
#[async_trait]
impl McpAuthorization for Reject {
  async fn approve(&self, _: McpApproval, _: CancellationToken) -> Result<()> {
    Err(RuntimeError::Tool("denied".into()))
  }
}

#[tokio::test]
async fn unsafe_urls_and_protocol_header_overrides_fail_before_connecting() {
  for url in [
    "http://example.com/mcp",
    "https://user:pass@example.com/mcp",
    "https://example.com/mcp?secret=1",
    "https://example.com/mcp#token",
    "file:///private/secret",
  ] {
    let config = McpServerConfig {
      id: "server".into(),
      transport: McpTransport::StreamableHttp {
        url: url.into(),
        headers: SecretHeaders::default(),
      },
      allowed_tools: ["echo".into()].into_iter().collect(),
    };
    assert!(matches!(
      McpManager::connect(vec![config], Arc::new(Reject), McpLimits::default()).await,
      Err(RuntimeError::Configuration(_))
    ));
  }
  let config = McpServerConfig {
    id: "server".into(),
    transport: McpTransport::StreamableHttp {
      url: "http://127.0.0.1:1/mcp".into(),
      headers: SecretHeaders(
        [("host".into(), "example.com".into())]
          .into_iter()
          .collect(),
      ),
    },
    allowed_tools: ["echo".into()].into_iter().collect(),
  };
  assert!(matches!(
    McpManager::connect(vec![config], Arc::new(Reject), McpLimits::default()).await,
    Err(RuntimeError::Configuration(_))
  ));
}

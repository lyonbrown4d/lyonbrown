use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_core::{AgentEvent, ToolCallId, ToolInvocation, ToolSource};
use agent_kernel::Kernel;
use agent_mcp::{McpBridge, McpServerConfig};
use agent_model::ModelRegistry;
use agent_secret::NoopBroker;
use agent_tool::{AllowAllApproval, ToolContext, ToolRegistry};
use provider_mock::{MockProvider, ScriptedStep};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn fixture() -> McpServerConfig {
  McpServerConfig {
    command: PathBuf::from(env!("CARGO_BIN_EXE_mcp_fixture")),
    args: vec![],
  }
}

#[tokio::test]
async fn lists_and_calls_stdio_tools_through_common_registry() {
  let bridge = McpBridge::connect("fixture", &fixture()).await.unwrap();
  let registry = ToolRegistry::new();
  bridge.register(&registry);
  let tool = registry.get("mcp.fixture.echo").unwrap();
  assert_eq!(
    tool.definition().source,
    ToolSource::Mcp {
      server: "fixture".into()
    }
  );
  let context = ToolContext::new(
    std::env::current_dir().unwrap(),
    Arc::new(NoopBroker),
    CancellationToken::new(),
    Duration::from_secs(60),
  );
  let result = tool
    .invoke(
      &context,
      &ToolInvocation {
        id: ToolCallId::new(),
        name: "mcp.fixture.echo".into(),
        arguments: json!({"text": "hello"}),
      },
    )
    .await
    .unwrap();
  assert!(result.ok);
  assert_eq!(result.output, "hello");
}

#[tokio::test]
async fn kernel_routes_mcp_tool_result_into_next_model_step() {
  let bridge = McpBridge::connect("fixture", &fixture()).await.unwrap();
  let tools = ToolRegistry::new();
  bridge.register(&tools);
  let registry = ModelRegistry::new();
  let provider = Arc::new(MockProvider::scripted(vec![
    ScriptedStep::tool_call("mcp.fixture.echo", json!({"text": "round trip"})),
    ScriptedStep::text("finished"),
  ]));
  registry.register(provider.clone());
  let mut kernel = Kernel::builder()
    .providers(registry)
    .tools(tools)
    .approval(AllowAllApproval)
    .build();

  let events = kernel.run_turn("call the MCP echo tool").await.unwrap();

  assert!(events.iter().any(|event| matches!(event,
    AgentEvent::ToolCompleted { result, .. } if result.ok && result.output == "round trip")));
  assert!(
    provider.requests()[1]
      .messages
      .iter()
      .any(|message| message.content == "round trip")
  );
}

#[tokio::test]
async fn kernel_survives_mcp_server_crash_and_returns_failed_observation() {
  let bridge = McpBridge::connect("fixture", &fixture()).await.unwrap();
  let tools = ToolRegistry::new();
  bridge.register(&tools);
  let registry = ModelRegistry::new();
  registry.register(Arc::new(MockProvider::scripted(vec![
    ScriptedStep::tool_call("mcp.fixture.crash", json!({})),
    ScriptedStep::text("continued"),
  ])));
  let mut kernel = Kernel::builder()
    .providers(registry)
    .tools(tools)
    .approval(AllowAllApproval)
    .build();
  let events = kernel.run_turn("run tool").await.unwrap();
  assert!(events.iter().any(|event| matches!(event,
    AgentEvent::ToolCompleted { result, .. } if !result.ok)));
  assert_eq!(
    events.iter().find_map(AgentEvent::turn_answer),
    Some("continued")
  );
}

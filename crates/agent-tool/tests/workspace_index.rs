use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use agent_core::{ToolCallId, ToolInvocation};
use agent_index::WorkspaceIndex;
use agent_secret::NoopBroker;
use agent_tool::{SearchWorkspaceTool, Tool, ToolContext};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::test]
async fn search_workspace_tool_returns_matches_from_local_index() {
  let root = std::env::temp_dir().join(format!("lyonbrown-search-{}", Uuid::new_v4()));
  fs::create_dir_all(root.join("src")).unwrap();
  fs::write(
    root.join("src/main.rs"),
    "fn workspace_index_search_target() {}",
  )
  .unwrap();
  let (index, watcher) =
    WorkspaceIndex::open_and_watch(&root, root.join("cache/index.db")).unwrap();
  let tool = SearchWorkspaceTool::new(index, watcher);
  let context = ToolContext::new(
    PathBuf::from(&root),
    Arc::new(NoopBroker),
    CancellationToken::new(),
    Duration::from_secs(1),
  );
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "search_workspace".into(),
    arguments: json!({"query": "workspace_index_search_target", "limit": 5}),
  };

  let result = tool.invoke(&context, &invocation).await.unwrap();

  assert!(result.ok);
  assert!(result.output.contains("src/main.rs"));
  assert!(result.output.contains("workspace_index_search_target"));
  drop(tool);
  drop(context);
  fs::remove_dir_all(root).unwrap();
}

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent_core::{RiskLevel, ToolCallId, ToolInvocation};
use agent_secret::NoopBroker;
use agent_tool::{
    ListFilesTool, ReadFileTool, ShellTool, Tool, ToolContext, ToolExecutionError, ToolExecutor,
    ToolRegistry, WriteFileTool,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("lyonbrown-m3-{}", ToolCallId::new()));
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

fn executor(root: &Path, tool: Arc<dyn Tool>, timeout: Duration) -> ToolExecutor {
    let registry = ToolRegistry::new();
    registry.register(tool);
    ToolExecutor::new(
        registry,
        ToolContext::new(
            root.to_path_buf(),
            Arc::new(NoopBroker),
            CancellationToken::new(),
            timeout,
        ),
    )
}

fn invocation(name: &str, arguments: serde_json::Value) -> ToolInvocation {
    ToolInvocation {
        id: ToolCallId::new(),
        name: name.into(),
        arguments,
    }
}

#[test]
fn builtin_risk_levels_match_the_approval_contract() {
    assert_eq!(ShellTool::new().definition().risk, RiskLevel::Execute);
    assert_eq!(WriteFileTool::new().definition().risk, RiskLevel::Write);
    assert_eq!(ReadFileTool::new().definition().risk, RiskLevel::Read);
    assert_eq!(ListFilesTool::new().definition().risk, RiskLevel::Read);
}

#[tokio::test]
async fn filesystem_tools_read_write_and_list_inside_workspace() {
    let root = TestDir::new();
    std::fs::create_dir(root.path().join("notes")).unwrap();

    let write = executor(
        root.path(),
        Arc::new(WriteFileTool::new()),
        Duration::from_secs(1),
    );
    let written = write
        .execute(&invocation(
            "write_file",
            json!({"path": "notes/todo.txt", "content": "one\ntwo\n"}),
        ))
        .await
        .unwrap();
    assert!(written.ok);

    let read = executor(
        root.path(),
        Arc::new(ReadFileTool::new()),
        Duration::from_secs(1),
    );
    let read = read
        .execute(&invocation("read_file", json!({"path": "notes/todo.txt"})))
        .await
        .unwrap();
    assert_eq!(read.output, "one\ntwo\n");

    let list = executor(
        root.path(),
        Arc::new(ListFilesTool::new()),
        Duration::from_secs(1),
    );
    let listed = list
        .execute(&invocation("list_files", json!({"path": "notes"})))
        .await
        .unwrap();
    assert_eq!(listed.output, "todo.txt");
}

#[tokio::test]
async fn every_filesystem_tool_rejects_parent_traversal() {
    let root = TestDir::new();
    for (name, tool, args) in [
        (
            "read_file",
            Arc::new(ReadFileTool::new()) as Arc<dyn Tool>,
            json!({"path": "../outside.txt"}),
        ),
        (
            "write_file",
            Arc::new(WriteFileTool::new()) as Arc<dyn Tool>,
            json!({"path": "../outside.txt", "content": "bad"}),
        ),
        (
            "list_files",
            Arc::new(ListFilesTool::new()) as Arc<dyn Tool>,
            json!({"path": ".."}),
        ),
    ] {
        let error = executor(root.path(), tool, Duration::from_secs(1))
            .execute(&invocation(name, args))
            .await
            .unwrap_err();
        assert!(
            matches!(error, ToolExecutionError::Tool(_)),
            "{name}: {error}"
        );
    }
}

#[tokio::test]
async fn shell_runs_in_workspace_and_caps_combined_output() {
    let root = TestDir::new();
    let shell = executor(
        root.path(),
        Arc::new(ShellTool::new()),
        Duration::from_secs(5),
    );
    let command = if cfg!(windows) {
        "cd & for /L %i in (1,1,40000) do @echo x"
    } else {
        "pwd; head -c 40000 /dev/zero | tr '\\0' x"
    };
    let result = shell
        .execute(&invocation("shell", json!({"command": command})))
        .await
        .unwrap();

    assert!(result.ok);
    assert!(result.truncated);
    assert!(result.output.len() <= 32 * 1024);
    assert!(result.output.contains(&root.path().display().to_string()));
}

#[tokio::test]
async fn shell_timeout_is_reported_by_the_executor() {
    let root = TestDir::new();
    let shell = executor(
        root.path(),
        Arc::new(ShellTool::new()),
        Duration::from_millis(50),
    );
    let command = if cfg!(windows) {
        "ping -n 10 127.0.0.1 >nul"
    } else {
        "sleep 10"
    };

    let error = shell
        .execute(&invocation("shell", json!({"command": command})))
        .await
        .unwrap_err();

    assert!(matches!(error, ToolExecutionError::TimedOut { .. }));
}

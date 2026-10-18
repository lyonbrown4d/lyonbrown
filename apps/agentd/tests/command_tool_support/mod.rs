mod replay;
use crate::support::TestNode;
use agent_protocol::v1;
use agent_test_support::{MockModelServer, StandardProtocol};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::OnceLock};

pub fn fixture() -> PathBuf {
  static DIRECTORY: OnceLock<tempfile::TempDir> = OnceLock::new();
  let directory = DIRECTORY.get_or_init(|| {
    let directory = tempfile::tempdir().unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("../../crates/agent-workspace/tests/process_support/fixture.rs");
    let result = std::process::Command::new("rustc")
      .args(["--edition=2024", "-O"])
      .arg(source)
      .arg("-o")
      .arg(
        directory
          .path()
          .join(format!("fixture{}", std::env::consts::EXE_SUFFIX)),
      )
      .output()
      .unwrap();
    assert!(
      result.status.success(),
      "fixture compilation failed: {}",
      String::from_utf8_lossy(&result.stderr)
    );
    directory
  });
  directory
    .path()
    .join(format!("fixture{}", std::env::consts::EXE_SUFFIX))
    .canonicalize()
    .unwrap()
}

pub fn route() -> String {
  json!({"task_kind":"code","model_policy":"general","allowed_tools":["execute_command"],
    "execution_mode":"local","target_hint":null,"risk_level":"high","confidence":0.99})
  .to_string()
}

pub fn command(arguments: Value) -> Value {
  json!({"executable":fixture(),"arguments":arguments,"cwd":".","timeout_ms":5000,"output_limit_bytes":16384})
}

pub async fn start(
  main: &MockModelServer,
  classifier: &MockModelServer,
  protocol: StandardProtocol,
) -> TestNode {
  TestNode::start_configured(main, classifier, protocol, |config| {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    std::fs::write(&path, format!("[commands]\nexecutables = [{}]\n[commands.environment]\nLYONBROWN_FIXTURE_SELECTED = \"FIXTURE_REFERENCE\"\n", json!(fixture()))).unwrap();
    let settings = agent_config::load_with_environment(directory.path(), Some(&path),
      &BTreeMap::from([("FIXTURE_REFERENCE".into(), "chosen".into())])).unwrap();
    config.configure_commands(&settings).unwrap();
  }).await
}

pub async fn approval(node: &TestNode, run: &str) -> v1::Approval {
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    let mut stream = node.client.watch_run(run, 0).await.unwrap();
    loop {
      if let Some(approval) = node.client.list_approvals(run).await.unwrap().first() {
        return approval.clone();
      }
      assert!(
        stream.message().await.unwrap().is_some(),
        "run ended before command approval"
      );
    }
  })
  .await
  .unwrap()
}

pub fn exact(approval: &v1::Approval) -> Value {
  serde_json::from_str(&approval.arguments_json).unwrap()
}

pub async fn wait_ready(path: &std::path::Path) {
  tokio::time::timeout(std::time::Duration::from_secs(3), async {
    while !path.exists() {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
}

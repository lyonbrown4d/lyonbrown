use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agent_cli::composition::{CompositionRoot, RuntimePaths};
use agent_cli::config::{CliOverrides, LoadOptions, RuntimeConfig};
use agent_tool::{ApprovalPolicy, SessionApprovalPolicy};

fn test_config(workspace: &std::path::Path) -> RuntimeConfig {
  RuntimeConfig::load(LoadOptions {
    global_config: workspace.join("missing-global.toml"),
    workspace_dir: workspace.to_path_buf(),
    explicit_config: None,
    environment: Some(HashMap::new()),
    cli: CliOverrides::default(),
  })
  .unwrap()
}

#[test]
fn core_bindings_are_root_singletons() {
  let workspace = PathBuf::from("test-workspace");
  let home = PathBuf::from("test-home");
  let approval: Arc<dyn ApprovalPolicy> = Arc::new(SessionApprovalPolicy::approve_all());
  let root = CompositionRoot::new(
    test_config(&workspace),
    RuntimePaths::new(workspace.clone(), home.clone()),
    approval,
  )
  .unwrap();

  let first = root.config().unwrap();
  let second = root.config().unwrap();
  assert!(Arc::ptr_eq(&first, &second));

  let paths = root.paths().unwrap();
  assert_eq!(paths.workspace(), workspace);
  assert_eq!(paths.home(), home);
}

#[tokio::test]
async fn bootstrap_registers_repository_services_and_builds_kernel() {
  let root_dir =
    std::env::temp_dir().join(format!("lyonbrown-composition-test-{}", std::process::id()));
  let workspace = root_dir.join("workspace");
  let home = root_dir.join("home");
  std::fs::create_dir_all(home.join(".agent").join("skills")).unwrap();
  std::fs::create_dir_all(&workspace).unwrap();
  let mut config = test_config(&workspace);
  config.model.provider = "ollama".into();
  config.storage.enabled = false;
  let approval: Arc<dyn ApprovalPolicy> = Arc::new(SessionApprovalPolicy::approve_all());

  let root = CompositionRoot::bootstrap(config, RuntimePaths::new(workspace, home), approval)
    .await
    .unwrap();

  root.validate().unwrap();
  let kernel = root.build_kernel().unwrap();
  drop(kernel);
  drop(root);
  std::fs::remove_dir_all(root_dir).unwrap();
}

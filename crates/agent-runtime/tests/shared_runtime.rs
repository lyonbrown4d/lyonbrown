use std::collections::HashMap;
use std::sync::Arc;

use agent_runtime::composition::{CompositionRoot, RuntimePaths};
use agent_runtime::config::{CliOverrides, LoadOptions, RuntimeConfig};
use agent_runtime::model_manager::default_preset;
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
fn shared_runtime_exposes_config_composition_and_model_catalog() {
  let workspace = std::path::PathBuf::from("test-workspace");
  let home = std::path::PathBuf::from("test-home");
  let approval: Arc<dyn ApprovalPolicy> = Arc::new(SessionApprovalPolicy::approve_all());

  let root = CompositionRoot::new(
    test_config(&workspace),
    RuntimePaths::new(workspace.clone(), home.clone()),
    approval,
  )
  .unwrap();

  assert_eq!(root.paths().unwrap().workspace(), workspace);
  assert_eq!(root.paths().unwrap().home(), home);
  assert_eq!(default_preset().id, "qwen3-1.7b-q4_k_m");
}

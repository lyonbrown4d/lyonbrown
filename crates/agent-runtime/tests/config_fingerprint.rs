use agent_runtime::config::{CliOverrides, LoadOptions, RuntimeConfig};
use std::collections::HashMap;

fn load(workspace: &std::path::Path, model: Option<&str>) -> RuntimeConfig {
  RuntimeConfig::load(LoadOptions {
    global_config: workspace.join("missing-global.toml"),
    workspace_dir: workspace.to_path_buf(),
    explicit_config: None,
    environment: Some(HashMap::new()),
    cli: CliOverrides {
      model: model.map(str::to_owned),
      ..CliOverrides::default()
    },
  })
  .expect("test runtime config")
}

#[test]
fn effective_runtime_fingerprint_is_stable_and_change_sensitive() {
  let workspace = std::path::Path::new("test-workspace");
  let first = load(workspace, None);
  let same = load(workspace, None);
  let changed = load(workspace, Some("another-model"));

  assert_eq!(first.fingerprint().unwrap(), same.fingerprint().unwrap());
  assert_ne!(first.fingerprint().unwrap(), changed.fingerprint().unwrap());
}

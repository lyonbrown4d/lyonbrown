use std::collections::HashMap;

use agent_runtime::config::{CliOverrides, LoadOptions, RuntimeConfig};

fn load(
  provider: &str,
  model: &str,
) -> Result<RuntimeConfig, agent_runtime::config::ConfigLoadError> {
  RuntimeConfig::load(LoadOptions {
    global_config: "missing-global.toml".into(),
    workspace_dir: "missing-workspace".into(),
    explicit_config: None,
    environment: Some(HashMap::new()),
    cli: CliOverrides {
      provider: Some(provider.into()),
      model: Some(model.into()),
      ..CliOverrides::default()
    },
  })
}

#[test]
fn configured_route_accepts_shared_length_limits() {
  let config = load(&"p".repeat(128), &"m".repeat(256)).unwrap();

  assert_eq!(config.model.provider.len(), 128);
  assert_eq!(config.model.model.unwrap().len(), 256);
}

#[test]
fn configured_route_rejects_empty_or_oversized_parts() {
  for (provider, model) in [
    ("provider".to_owned(), String::new()),
    ("p".repeat(129), "model".to_owned()),
    ("provider".to_owned(), "m".repeat(257)),
  ] {
    let error = load(&provider, &model).unwrap_err();
    assert!(matches!(
      error,
      agent_runtime::config::ConfigLoadError::Validation(_)
    ));
  }
}

use super::*;

#[test]
fn invalid_values_are_reported() {
  let root = TestDir::new();
  let invalid = root.write("invalid.toml", "[model]\nmax_tokens = 'many'\n");

  let error = RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    explicit_config: Some(invalid),
    environment: Some(HashMap::new()),
    ..Default::default()
  })
  .unwrap_err();

  assert!(error.to_string().contains("max_tokens"));
}

#[test]
fn unsupported_explicit_extension_is_reported() {
  let root = TestDir::new();
  let unsupported = root.write("agent.txt", "model.provider=bad");

  let error = RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    explicit_config: Some(unsupported),
    environment: Some(HashMap::new()),
    ..Default::default()
  })
  .unwrap_err();

  assert!(error.to_string().contains("supported file format"));
}

#[test]
fn secret_plaintext_fields_are_rejected() {
  let root = TestDir::new();
  let invalid = root.write("secret.toml", "api_key = 'plaintext-secret'\n");

  let error = RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    explicit_config: Some(invalid),
    environment: Some(HashMap::new()),
    ..Default::default()
  })
  .unwrap_err();

  assert!(error.to_string().contains("api_key"));
}

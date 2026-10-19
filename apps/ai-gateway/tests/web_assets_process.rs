use std::process::Command;

#[test]
fn startup_without_hydration_assets_fails_before_database_bootstrap() {
  let directory = tempfile::tempdir().expect("tempdir");
  let config = directory.path().join("gateway.toml");
  std::fs::write(
    &config,
    r#"
[gateway]
token = "independent-web-assets-api-token-at-least-32-bytes"
[[gateway.models]]
id = "local"
upstream_model = "fixture"
api_base = "http://127.0.0.1:1234/v1"
tier = "balanced"
local_inference = true
capabilities = ["text"]
context_tokens = 65536
"#,
  )
  .expect("configuration");
  std::fs::write(
    directory.path().join(".env"),
    "LYONBROWN_GATEWAY_ADMIN_USERNAME=fixture-admin\nLYONBROWN_GATEWAY_ADMIN_PASSWORD=private-web-assets-password\n",
  )
  .expect("runtime credentials");
  let data = directory.path().join("data");
  let output = Command::new(env!("CARGO_BIN_EXE_ai-gateway"))
    .env_clear()
    .env(
      "LYONBROWN_GATEWAY_WEB_ROOT",
      directory.path().join("absent-site"),
    )
    .arg("--config")
    .arg(config)
    .arg("--data-dir")
    .arg(&data)
    .output()
    .expect("Gateway process");
  assert_eq!(output.status.code(), Some(1));
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("gateway WebUI assets are missing"));
  assert!(stderr.contains("gateway-dev --build-only"));
  for forbidden in [
    "panicked",
    "private-web-assets-password",
    "independent-web-assets-api-token",
  ] {
    assert!(!stderr.contains(forbidden));
  }
  assert!(!data.join("gateway.sqlite").exists());
}

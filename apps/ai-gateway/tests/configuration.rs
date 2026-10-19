use ai_gateway::config::AppConfig;
use std::collections::BTreeMap;

const SETTINGS: &str = r#"
[gateway]
token = "configuration-file-secret-at-least-32-bytes"
[[gateway.models]]
id = "local"
upstream_model = "actual"
api_base = "http://127.0.0.1:1234/v1"
tier = "balanced"
local_inference = true
capabilities = ["text", "tools"]
context_tokens = 65536
api_key_env = "GATEWAY_TEST_PROVIDER_KEY"
"#;
const USERNAME: &str = "LYONBROWN_GATEWAY_ADMIN_USERNAME";
const PASSWORD: &str = "LYONBROWN_GATEWAY_ADMIN_PASSWORD";
fn environment() -> BTreeMap<String, String> {
  BTreeMap::from([
    (USERNAME.into(), "test-admin@example.local".into()),
    (PASSWORD.into(), "environment-private-password".into()),
    (
      "GATEWAY_TEST_PROVIDER_KEY".into(),
      "provider-credential".into(),
    ),
  ])
}
#[test]
fn file_dotenv_environment_layers_and_secret_redaction() {
  let dir = tempfile::tempdir().expect("tempdir");
  let path = dir.path().join("gateway.toml");
  std::fs::write(&path, SETTINGS).expect("write settings");
  std::fs::write(
    dir.path().join(".env"),
    "LYONBROWN_GATEWAY_LISTEN=127.0.0.1:8100\nLYONBROWN_GATEWAY_TOKEN=dotenv-secret-with-at-least-32-bytes\nLYONBROWN_GATEWAY_ADMIN_USERNAME=dotenv-admin\nLYONBROWN_GATEWAY_ADMIN_PASSWORD=dotenv-private-password\nGATEWAY_TEST_PROVIDER_KEY=dotenv-provider-credential\n",
  ).expect("write dotenv");
  let dotenv = AppConfig::load_with_environment(dir.path(), Some(&path), &BTreeMap::new())
    .expect("dotenv supplies required credentials");
  assert_eq!(dotenv.admin_username, "dotenv-admin");
  assert_eq!(
    dotenv.admin_password.expose_secret(),
    "dotenv-private-password"
  );
  assert_eq!(dotenv.listen.port(), 8100);
  let data_override = dir.path().join("runtime-data");
  let mut env = environment();
  env.extend([
    (
      "LYONBROWN_GATEWAY_DATA_DIR".into(),
      data_override.to_str().expect("path").into(),
    ),
    ("LYONBROWN_GATEWAY_LISTEN".into(), "127.0.0.1:8101".into()),
    (
      "LYONBROWN_GATEWAY_TOKEN".into(),
      "environment-secret-with-at-least-32-bytes".into(),
    ),
  ]);
  let settings =
    AppConfig::load_with_environment(dir.path(), Some(&path), &env).expect("load settings");
  assert_eq!(settings.listen.port(), 8101);
  assert_eq!(settings.data_dir, data_override);
  assert_eq!(settings.admin_username, "test-admin@example.local");
  assert_eq!(
    settings.admin_password.expose_secret(),
    "environment-private-password"
  );
  assert_eq!(
    settings.gateway.models[0]
      .api_key
      .as_ref()
      .expect("resolved credential")
      .expose_secret(),
    "provider-credential"
  );
  let debug = format!("{:?} {:?}", settings.gateway, settings.admin_password);
  for secret in [
    "configuration-file-secret",
    "dotenv-secret",
    "environment-secret",
    "environment-private-password",
    "provider-credential",
  ] {
    assert!(!debug.contains(secret));
  }
}
#[test]
fn credentials_are_required_validated_and_never_echoed() {
  let dir = tempfile::tempdir().expect("tempdir");
  let path = dir.path().join("gateway.toml");
  std::fs::write(&path, SETTINGS).expect("write settings");
  for key in [USERNAME, PASSWORD] {
    let mut env = environment();
    env.remove(key);
    // The removed legacy credential cannot supply either required setting.
    env.insert(
      "LYONBROWN_GATEWAY_ADMIN_TOKEN".into(),
      "legacy-admin-secret".into(),
    );
    assert!(AppConfig::load_with_environment(dir.path(), Some(&path), &env).is_err());
  }
  for (key, value) in [
    (USERNAME, String::new()),
    (USERNAME, "a".repeat(65)),
    (USERNAME, "admin/name".into()),
    (USERNAME, "管理员".into()),
    (PASSWORD, "short-password".into()),
    (PASSWORD, "p".repeat(129)),
    (
      PASSWORD,
      "configuration-file-secret-at-least-32-bytes".into(),
    ),
  ] {
    let mut env = environment();
    env.insert(key.into(), value.clone());
    let error = AppConfig::load_with_environment(dir.path(), Some(&path), &env)
      .err()
      .expect("invalid credentials rejected")
      .to_string();
    assert!(!error.contains("environment-private-password"));
    if !value.is_empty() {
      assert!(!error.contains(&value));
    }
  }
  for (username, password) in [
    ("a".into(), "p".repeat(15)),
    ("a".repeat(64), "p".repeat(128)),
  ] {
    let mut env = environment();
    env.insert(USERNAME.into(), username);
    env.insert(PASSWORD.into(), password);
    assert!(AppConfig::load_with_environment(dir.path(), Some(&path), &env).is_ok());
  }
}
#[test]
fn administrator_credentials_in_configuration_files_are_rejected() {
  let dir = tempfile::tempdir().expect("tempdir");
  let path = dir.path().join("gateway.toml");
  for field in ["admin_username", "admin_password", "admin_token"] {
    std::fs::write(
      &path,
      format!("{field} = 'file-private-admin-value'\n{SETTINGS}"),
    )
    .expect("write settings");
    let error = AppConfig::load_with_environment(dir.path(), Some(&path), &environment())
      .err()
      .expect("file credentials rejected despite valid environment");
    assert!(!error.to_string().contains("file-private-admin-value"));
  }
}
#[test]
fn invalid_settings_fail_without_echoing_credentials() {
  let dir = tempfile::tempdir().expect("tempdir");
  let path = dir.path().join("gateway.toml");
  std::fs::write(&path, SETTINGS).expect("write settings");
  let mut env = environment();
  env.insert("LYONBROWN_GATEWAY_LISTEN".into(), "0.0.0.0:8101".into());
  let error = AppConfig::load_with_environment(dir.path(), Some(&path), &env)
    .err()
    .expect("rejected listener");
  assert!(!error.to_string().contains("configuration-file-secret"));
  std::fs::write(&path, "gateway = 'secret-parser-input'").expect("write invalid");
  let error = AppConfig::load_with_environment(dir.path(), Some(&path), &BTreeMap::new())
    .err()
    .expect("reject shape");
  assert!(!error.to_string().contains("secret-parser-input"));
}

#[test]
fn web_root_layers_are_absolute_and_startup_requires_both_hydration_assets() {
  let dir = tempfile::tempdir().expect("tempdir");
  let path = dir.path().join("gateway.toml");
  std::fs::write(&path, SETTINGS).expect("settings");
  let dotenv_root = dir.path().join("dotenv-web");
  std::fs::write(
    dir.path().join(".env"),
    format!("LYONBROWN_GATEWAY_WEB_ROOT={}\n", dotenv_root.display()),
  )
  .expect("dotenv");
  let dotenv = AppConfig::load_with_environment(dir.path(), Some(&path), &environment())
    .expect("dotenv WebUI root");
  assert_eq!(dotenv.web_root, dotenv_root);
  let web_root = dir.path().join("process-web");
  let mut env = environment();
  env.insert(
    "LYONBROWN_GATEWAY_WEB_ROOT".into(),
    web_root.to_str().expect("UTF-8").into(),
  );
  let settings =
    AppConfig::load_with_environment(dir.path(), Some(&path), &env).expect("WebUI root");
  assert_eq!(settings.web_root, web_root);
  assert!(settings.validate_web_assets().is_err());
  let package = web_root.join("ui/pkg");
  std::fs::create_dir_all(&package).expect("package directory");
  std::fs::write(
    package.join("gateway_web.js"),
    "export default function init() {};",
  )
  .expect("JS");
  assert!(settings.validate_web_assets().is_err());
  std::fs::write(package.join("gateway_web.wasm"), b"\0asm\x01\0\0\0").expect("WASM");
  assert!(settings.validate_web_assets().is_ok());
  std::fs::write(package.join("gateway_web.wasm"), b"").expect("empty WASM");
  assert!(settings.validate_web_assets().is_err());
  env.insert("LYONBROWN_GATEWAY_WEB_ROOT".into(), "relative-web".into());
  assert!(AppConfig::load_with_environment(dir.path(), Some(&path), &env).is_err());
}

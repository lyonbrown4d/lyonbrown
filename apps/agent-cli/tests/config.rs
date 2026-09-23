use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_cli::config::{CliOverrides, LoadOptions, RuntimeConfig};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct TestDir(PathBuf);

impl TestDir {
  fn new() -> Self {
    let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let path =
      std::env::temp_dir().join(format!("lyonbrown-config-test-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    Self(path)
  }

  fn path(&self) -> &Path {
    &self.0
  }

  fn write(&self, name: &str, contents: &str) -> PathBuf {
    let path = self.0.join(name);
    std::fs::write(&path, contents).unwrap();
    path
  }
}

impl Drop for TestDir {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

#[test]
fn precedence_is_defaults_global_workspace_explicit_environment_cli() {
  let global = TestDir::new();
  let workspace = TestDir::new();
  let global_file = global.write(
    "config.toml",
    "[model]\nprovider = 'global'\nmodel = 'global-model'\n",
  );
  workspace.write(
    "agent.toml",
    "[model]\nprovider = 'workspace'\nmodel = 'workspace-model'\n",
  );
  let explicit = workspace.write(
    "override.yaml",
    "model:\n  provider: explicit\nollama:\n  endpoint: http://explicit:11434\n",
  );
  let environment = HashMap::from([
    ("LYONBROWN_MODEL__PROVIDER".into(), "environment".into()),
    ("LYONBROWN_MODEL__MAX_TOKENS".into(), "99".into()),
  ]);

  let loaded = RuntimeConfig::load(LoadOptions {
    global_config: global_file,
    workspace_dir: workspace.path().to_owned(),
    explicit_config: Some(explicit),
    environment: Some(environment),
    cli: CliOverrides {
      provider: Some("cli".into()),
      approve_all: Some(true),
      trust_workspace_config: true,
      ..Default::default()
    },
  })
  .unwrap();

  assert_eq!(loaded.model.provider, "cli");
  assert_eq!(loaded.model.model.as_deref(), Some("workspace-model"));
  assert_eq!(loaded.model.max_tokens, 99);
  assert_eq!(loaded.ollama.endpoint, "http://explicit:11434");
  assert!(loaded.approval.approve_all);
}

#[test]
fn missing_optional_global_and_workspace_files_use_typed_defaults() {
  let root = TestDir::new();

  let loaded = RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    environment: Some(HashMap::new()),
    ..Default::default()
  })
  .unwrap();

  assert_eq!(loaded.model.provider, "llamacpp");
  assert_eq!(loaded.model.thinking, Some(false));
  assert_eq!(loaded.profile, "default-dev");
  assert_eq!(loaded.ollama.endpoint, "http://127.0.0.1:11434");
  assert!(loaded.llamacpp.model_path.is_none());
  assert_eq!(loaded.llamacpp.context_size, 4_096);
  assert_eq!(loaded.llamacpp.max_output_tokens, 2_048);
  assert_eq!(loaded.llamacpp.batch_size, 512);
  assert!(!loaded.approval.approve_all);
  assert!(!loaded.approval.deny_all);
}

#[test]
fn llamacpp_configuration_supports_files_and_environment() {
  let root = TestDir::new();
  root.write(
    "agent.toml",
    "[model]\nprovider = 'llamacpp'\nmodel = 'qwen-local'\n\
         [llamacpp]\nmodel_path = 'models/qwen.gguf'\ncontext_size = 8192\nthreads = 6\n",
  );
  let environment = HashMap::from([
    ("LYONBROWN_LLAMACPP__GPU_LAYERS".into(), "24".into()),
    (
      "LYONBROWN_LLAMACPP__MAX_OUTPUT_TOKENS".into(),
      "1024".into(),
    ),
  ]);

  let loaded = RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    environment: Some(environment),
    cli: CliOverrides {
      trust_workspace_config: true,
      ..Default::default()
    },
    ..Default::default()
  })
  .unwrap();

  assert_eq!(loaded.model.provider, "llamacpp");
  assert_eq!(loaded.model.model.as_deref(), Some("qwen-local"));
  assert_eq!(
    loaded.llamacpp.model_path.as_deref(),
    Some(Path::new("models/qwen.gguf"))
  );
  assert_eq!(loaded.llamacpp.context_size, 8_192);
  assert_eq!(loaded.llamacpp.max_output_tokens, 1_024);
  assert_eq!(loaded.llamacpp.threads, 6);
  assert_eq!(loaded.llamacpp.gpu_layers, 24);
}

#[test]
fn selected_llamacpp_uses_automatic_model_selection_without_a_path() {
  let root = TestDir::new();
  root.write("agent.toml", "[model]\nprovider = 'llamacpp'\n");

  let loaded = RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    environment: Some(HashMap::new()),
    cli: CliOverrides {
      trust_workspace_config: true,
      ..Default::default()
    },
    ..Default::default()
  })
  .unwrap();

  assert_eq!(loaded.model.provider, "llamacpp");
  assert!(loaded.llamacpp.model_path.is_none());
}

#[test]
fn llamacpp_output_limit_must_fit_context_window() {
  let root = TestDir::new();
  let invalid = root.write(
    "invalid.toml",
    "[model]\nprovider = 'llamacpp'\n[llamacpp]\nmodel_path = 'model.gguf'\n\
         context_size = 128\nmax_output_tokens = 256\n",
  );

  let error = load_explicit(&root, invalid).unwrap_err();

  assert!(error.to_string().contains("max_output_tokens"));
}

#[test]
fn profile_has_typed_config_and_cli_precedence() {
  let root = TestDir::new();
  root.write("agent.toml", "profile = 'workspace-profile'\n");
  let loaded = RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    environment: Some(HashMap::new()),
    cli: CliOverrides {
      profile: Some("cli-profile".into()),
      trust_workspace_config: true,
      ..Default::default()
    },
    ..Default::default()
  })
  .unwrap();
  assert_eq!(loaded.profile, "cli-profile");
}

#[test]
fn approval_flags_must_not_conflict() {
  let root = TestDir::new();
  let error = RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    environment: Some(HashMap::new()),
    cli: CliOverrides {
      approve_all: Some(true),
      deny_all: Some(true),
      ..Default::default()
    },
    ..Default::default()
  })
  .unwrap_err();

  assert!(error.to_string().contains("approve_all"));
}

#[test]
fn explicit_json_configuration_is_supported() {
  let root = TestDir::new();
  let explicit = root.write(
    "agent.json",
    r#"{"model":{"provider":"json","max_tokens":1234}}"#,
  );

  let loaded = RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    explicit_config: Some(explicit),
    environment: Some(HashMap::new()),
    ..Default::default()
  })
  .unwrap();

  assert_eq!(loaded.model.provider, "json");
  assert_eq!(loaded.model.max_tokens, 1_234);
}

#[test]
fn explicit_json5_configuration_is_supported() {
  let root = TestDir::new();
  let explicit = root.write(
    "agent.json5",
    "{ model: { provider: 'json5', max_tokens: 123 } }",
  );

  let loaded = load_explicit(&root, explicit).unwrap();

  assert_eq!(loaded.model.provider, "json5");
  assert_eq!(loaded.model.max_tokens, 123);
}

#[test]
fn explicit_ron_configuration_is_supported() {
  let root = TestDir::new();
  let explicit = root.write("agent.ron", "(model: (provider: \"ron\", max_tokens: 234))");

  let loaded = load_explicit(&root, explicit).unwrap();

  assert_eq!(loaded.model.provider, "ron");
  assert_eq!(loaded.model.max_tokens, 234);
}

#[test]
fn explicit_ini_configuration_is_supported() {
  let root = TestDir::new();
  let explicit = root.write("agent.ini", "[model]\nprovider=ini\nmax_tokens=345\n");

  let loaded = load_explicit(&root, explicit).unwrap();

  assert_eq!(loaded.model.provider, "ini");
  assert_eq!(loaded.model.max_tokens, 345);
}

fn load_explicit(
  root: &TestDir,
  explicit: PathBuf,
) -> Result<RuntimeConfig, agent_cli::config::ConfigLoadError> {
  RuntimeConfig::load(LoadOptions {
    global_config: root.path().join("missing.toml"),
    workspace_dir: root.path().to_owned(),
    explicit_config: Some(explicit),
    environment: Some(HashMap::new()),
    ..Default::default()
  })
}

#[path = "config/extended.rs"]
mod extended;

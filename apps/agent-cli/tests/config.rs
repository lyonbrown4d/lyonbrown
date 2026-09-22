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

    assert_eq!(loaded.model.provider, "ollama");
    assert_eq!(loaded.ollama.endpoint, "http://127.0.0.1:11434");
    assert!(!loaded.approval.approve_all);
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

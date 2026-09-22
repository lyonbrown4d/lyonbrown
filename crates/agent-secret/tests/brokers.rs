use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_core::SecretRef;
use agent_secret::{CompositeBroker, EnvSecretBroker, FileSecretBroker, SecretBroker, SecretError};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lyonbrown-m5-secret-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn environment_broker_resolves_and_redacts_only_resolved_values() {
    let expected = std::env::var("PATH").expect("test process must have PATH");
    let broker = EnvSecretBroker::new();

    let value = broker.resolve(&SecretRef::env("PATH")).await.unwrap();

    assert_eq!(value, expected);
    assert_eq!(
        broker.redact(&format!("before:{value}:after")),
        "before:[REDACTED]:after"
    );
    assert_eq!(broker.redact("unrelated"), "unrelated");
}

#[tokio::test]
async fn file_broker_supports_bare_and_key_value_files() {
    let root = TestDir::new();
    let broker = FileSecretBroker::new(root.path()).unwrap();
    broker.store("bare", "bare-secret\n").unwrap();
    broker.store("pair", "TOKEN=pair-secret\r\n").unwrap();

    assert_eq!(
        broker.resolve(&SecretRef::file("bare")).await.unwrap(),
        "bare-secret"
    );
    assert_eq!(
        broker.resolve(&SecretRef::file("pair")).await.unwrap(),
        "pair-secret"
    );
    assert_eq!(
        broker.redact("bare-secret + pair-secret"),
        "[REDACTED] + [REDACTED]"
    );
}

#[tokio::test]
async fn file_broker_rejects_traversal_and_reports_missing_secrets() {
    let root = TestDir::new();
    let broker = FileSecretBroker::new(root.path()).unwrap();

    let traversal = broker.resolve(&SecretRef::file("../outside")).await;
    let missing = broker.resolve(&SecretRef::file("missing")).await;

    assert!(matches!(traversal, Err(SecretError::InvalidReference(_))));
    assert!(matches!(missing, Err(SecretError::NotFound(_))));
}

#[cfg(unix)]
#[test]
fn file_broker_stores_secrets_with_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let root = TestDir::new();
    let broker = FileSecretBroker::new(root.path()).unwrap();

    broker.store("token", "value").unwrap();

    let mode = std::fs::metadata(root.path().join("token"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[tokio::test]
async fn composite_routes_by_reference_and_redacts_overlapping_values_safely() {
    let root = TestDir::new();
    let files = FileSecretBroker::new(root.path()).unwrap();
    files.store("short", "token").unwrap();
    files.store("long", "token-suffix").unwrap();
    files.store("marker", "REDACTED").unwrap();
    let composite = CompositeBroker::new(vec![
        Arc::new(EnvSecretBroker::new()) as Arc<dyn SecretBroker>,
        Arc::new(files),
    ]);

    composite.resolve(&SecretRef::file("short")).await.unwrap();
    composite.resolve(&SecretRef::file("long")).await.unwrap();
    composite.resolve(&SecretRef::file("marker")).await.unwrap();

    assert_eq!(
        composite.redact("token-suffix token"),
        "[REDACTED] [REDACTED]"
    );
    assert_eq!(composite.redact("REDACTED"), "[REDACTED]");
}

#[tokio::test]
async fn composite_routing_does_not_depend_on_broker_order() {
    let root = TestDir::new();
    let composite = CompositeBroker::new(vec![
        Arc::new(FileSecretBroker::new(root.path()).unwrap()) as Arc<dyn SecretBroker>,
        Arc::new(EnvSecretBroker::new()),
    ]);

    let value = composite.resolve(&SecretRef::env("PATH")).await.unwrap();

    assert_eq!(value, std::env::var("PATH").unwrap());
}

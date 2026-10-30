use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use agent_client::local_state::LocalStateStore;
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct UiState {
  selected_tab: String,
  expanded: Vec<String>,
}

fn state(tab: &str) -> UiState {
  UiState {
    selected_tab: tab.into(),
    expanded: vec!["history".into()],
  }
}

fn store(root: &std::path::Path, client: &str, endpoint: &str, node: &str) -> LocalStateStore {
  LocalStateStore::new(root.to_path_buf(), client, endpoint, node).expect("store should open")
}

fn state_file(root: &std::path::Path) -> std::path::PathBuf {
  fs::read_dir(root.join("client-state"))
    .expect("state directory")
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .find(|path| {
      path
        .extension()
        .is_some_and(|extension| extension == "json")
    })
    .expect("snapshot file")
}

#[test]
fn round_trips_across_store_recreation() {
  let root = tempdir().expect("temporary directory");
  {
    let first = store(root.path(), "tui", "http://localhost:4210", "node-a");
    first.save(&state("sessions")).expect("save snapshot");
  }
  let reopened = store(root.path(), "tui", "http://localhost:4210/", "node-a");
  assert_eq!(
    reopened.load::<UiState>().expect("load snapshot"),
    Some(state("sessions"))
  );
}

#[test]
fn canonical_connection_identity_matches_the_store_scope() {
  use agent_client::local_state::identity;
  let scope = identity("desktop", "https://daemon.example/", "node-a").expect("scope");
  assert_eq!(
    scope,
    identity("desktop", "HTTPS://DAEMON.EXAMPLE:443", "node-a").expect("equivalent scope")
  );
  assert_ne!(
    scope,
    identity("desktop", "https://daemon.example", "node-b").expect("other node")
  );
  assert_ne!(
    scope,
    identity("tui", "https://daemon.example", "node-a").expect("other client")
  );
  assert!(identity("desktop", "https://user:secret@daemon.example", "node-a").is_err());
}

#[test]
fn endpoint_node_and_client_kind_are_isolated() {
  let root = tempdir().expect("temporary directory");
  store(root.path(), "tui", "https://daemon.example:443", "node-a")
    .save(&state("one"))
    .expect("save snapshot");
  for (client, endpoint, node) in [
    ("tui", "https://daemon.example:443", "node-b"),
    ("tui", "https://other.example", "node-a"),
    ("desktop", "https://daemon.example:443", "node-a"),
  ] {
    assert_eq!(
      store(root.path(), client, endpoint, node)
        .load::<UiState>()
        .expect("load isolated state"),
      None
    );
  }
}

#[test]
fn malformed_future_and_oversized_files_are_errors_and_preserved() {
  let root = tempdir().expect("temporary directory");
  let store = store(root.path(), "tui", "http://localhost:4210", "node-a");
  store.save(&state("original")).expect("initial snapshot");
  let path = state_file(root.path());
  for bytes in [
    br#"{"schema_version":1,"identity":"wrong","payload":{}}"#.to_vec(),
    br#"{"schema_version":99,"identity":"x","payload":{}}"#.to_vec(),
    b"{malformed".to_vec(),
    vec![b'x'; 4 * 1024 * 1024 + 1],
  ] {
    fs::write(&path, &bytes).expect("replace fixture");
    assert!(store.load::<UiState>().is_err());
    assert_eq!(fs::read(&path).expect("preserved bytes"), bytes);
    assert!(store.save(&state("replacement")).is_err());
    assert_eq!(
      fs::read(&path).expect("preserved bytes after failed save"),
      bytes
    );
  }
}

#[cfg(unix)]
#[test]
fn directory_and_snapshot_have_private_permissions() {
  let root = tempdir().expect("temporary directory");
  let store = store(root.path(), "desktop", "http://localhost", "node-a");
  store.save(&state("home")).expect("save snapshot");
  let directory_mode = fs::metadata(root.path().join("client-state"))
    .expect("state directory metadata")
    .permissions()
    .mode()
    & 0o777;
  let file_mode = fs::metadata(state_file(root.path()))
    .expect("snapshot metadata")
    .permissions()
    .mode()
    & 0o777;
  assert_eq!(directory_mode, 0o700);
  assert_eq!(file_mode, 0o600);
}

#[cfg(unix)]
#[test]
fn symlink_state_target_is_rejected_without_touching_target() {
  use std::os::unix::fs::symlink;
  let root = tempdir().expect("temporary directory");
  let first = store(root.path(), "tui", "http://localhost", "node-a");
  first.save(&state("safe")).expect("initial snapshot");
  let file = state_file(root.path());
  drop(first);
  let outside = root.path().join("outside.json");
  fs::write(&outside, b"outside").expect("outside file");
  fs::remove_file(&file).expect("remove snapshot");
  symlink(&outside, &file).expect("create symlink");
  let reopened = store(root.path(), "tui", "http://localhost", "node-a");
  assert!(reopened.load::<UiState>().is_err());
  assert!(reopened.save(&state("overwrite")).is_err());
  assert_eq!(fs::read(outside).expect("outside contents"), b"outside");
}

#[cfg(unix)]
#[test]
fn symlink_lock_is_rejected() {
  use std::os::unix::fs::symlink;
  let root = tempdir().expect("temporary directory");
  let initial = store(root.path(), "tui", "http://localhost", "node-a");
  initial.save(&state("safe")).expect("save snapshot");
  let lock = fs::read_dir(root.path().join("client-state"))
    .expect("state directory")
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .find(|path| {
      path
        .extension()
        .is_some_and(|extension| extension == "lock")
    })
    .expect("lock file");
  drop(initial);
  let outside = root.path().join("outside.lock");
  fs::write(&outside, b"outside").expect("outside lock");
  fs::remove_file(&lock).expect("remove lock");
  symlink(&outside, &lock).expect("create lock symlink");
  assert!(
    LocalStateStore::new(
      root.path().to_path_buf(),
      "tui",
      "http://localhost",
      "node-a"
    )
    .is_err()
  );
  assert_eq!(
    fs::read(outside).expect("outside lock contents"),
    b"outside"
  );
}

#[test]
fn exclusive_ownership_is_released_on_drop() {
  let root = tempdir().expect("temporary directory");
  let owner = store(root.path(), "tui", "http://localhost", "node-a");
  assert!(
    LocalStateStore::new(
      root.path().to_path_buf(),
      "tui",
      "http://localhost",
      "node-a"
    )
    .is_err()
  );
  drop(owner);
  assert!(
    LocalStateStore::new(
      root.path().to_path_buf(),
      "tui",
      "http://localhost",
      "node-a"
    )
    .is_ok()
  );
}

#[test]
fn rejects_unsupported_kinds_and_unsafe_urls() {
  let root = tempdir().expect("temporary directory");
  for (kind, endpoint, node) in [
    ("other", "http://localhost", "node"),
    ("tui", "ftp://localhost", "node"),
    ("tui", "https://user:secret@daemon.example", "node"),
    ("tui", "https://daemon.example?token=x", "node"),
    ("tui", "https://daemon.example#fragment", "node"),
    ("tui", "http://localhost", "  "),
  ] {
    assert!(LocalStateStore::new(root.path().to_path_buf(), kind, endpoint, node).is_err());
  }
}

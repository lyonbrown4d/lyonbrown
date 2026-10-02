#![cfg(unix)]

use agent_local::{
  DiscoveryDescriptor, DiscoveryError, DiscoveryStore, LifecycleLock, ScopeFingerprint,
  SecretToken, SecureRunDirectory,
};
use std::net::SocketAddr;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use uuid::Uuid;

fn descriptor(pid: u32) -> DiscoveryDescriptor {
  DiscoveryDescriptor::new(
    "127.0.0.1:41000".parse::<SocketAddr>().expect("address"),
    pid,
    Uuid::new_v4(),
    SecretToken::generate().expect("token"),
    ScopeFingerprint::parse(&"ab".repeat(32)).expect("scope"),
  )
  .expect("descriptor")
}

#[test]
fn creates_a_fresh_nested_run_directory_with_private_modes() {
  let root = tempfile::tempdir().expect("temporary directory");
  let first = root.path().join("cache");
  let second = first.join("scope");
  let run = second.join("run");

  let directory = SecureRunDirectory::open(&run).expect("nested run directory");

  assert_eq!(directory.path(), run);
  for path in [first, second, run] {
    let mode = std::fs::metadata(path)
      .expect("directory metadata")
      .permissions()
      .mode();
    assert_eq!(mode & 0o777, 0o700);
  }
}

#[test]
fn rejects_a_symlink_in_a_nested_run_directory_path() {
  let root = tempfile::tempdir().expect("temporary directory");
  let outside = root.path().join("outside");
  std::fs::create_dir(&outside).expect("outside directory");
  std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o700))
    .expect("outside permissions");
  let linked_parent = root.path().join("linked-parent");
  symlink(&outside, &linked_parent).expect("parent symlink");

  assert!(matches!(
    SecureRunDirectory::open(linked_parent.join("run")),
    Err(agent_local::DiscoveryError::SymlinkRejected(path)) if path == linked_parent
  ));
}

#[test]
fn descriptor_io_remains_bound_to_the_open_directory_after_path_replacement() {
  let root = tempfile::tempdir().expect("temporary directory");
  let run = root.path().join("run");
  let detached = root.path().join("detached");
  let store = DiscoveryStore::new(SecureRunDirectory::open(&run).expect("run directory"));
  let first = descriptor(101);
  store.publish(&first).expect("first descriptor");

  std::fs::rename(&run, &detached).expect("detach run directory");
  std::fs::create_dir(&run).expect("replacement directory");
  std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).expect("permissions");
  let replacement = descriptor(202);
  std::fs::write(
    run.join("agentd.json"),
    replacement.to_json().expect("replacement json"),
  )
  .expect("replacement descriptor");
  std::fs::set_permissions(
    run.join("agentd.json"),
    std::fs::Permissions::from_mode(0o600),
  )
  .expect("replacement permissions");

  assert_eq!(store.read().expect("anchored read").pid(), first.pid());
  let updated = descriptor(303);
  store.publish(&updated).expect("anchored publish");
  assert_eq!(
    store.read().expect("updated anchored read").pid(),
    updated.pid()
  );

  let replacement_bytes = std::fs::read(run.join("agentd.json")).expect("replacement bytes");
  assert_eq!(
    DiscoveryDescriptor::from_json(&replacement_bytes)
      .expect("replacement descriptor")
      .pid(),
    replacement.pid()
  );
  let detached_bytes = std::fs::read(detached.join("agentd.json")).expect("detached bytes");
  assert_eq!(
    DiscoveryDescriptor::from_json(&detached_bytes)
      .expect("detached descriptor")
      .pid(),
    updated.pid()
  );
}

#[test]
fn lifecycle_lock_remains_bound_to_the_open_directory_after_path_replacement() {
  let root = tempfile::tempdir().expect("temporary directory");
  let run = root.path().join("run");
  let detached = root.path().join("detached");
  let directory = SecureRunDirectory::open(&run).expect("run directory");

  std::fs::rename(&run, &detached).expect("detach run directory");
  std::fs::create_dir(&run).expect("replacement directory");
  std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).expect("permissions");
  let _lock = LifecycleLock::try_acquire(&directory)
    .expect("lock attempt")
    .expect("lock owner");

  assert!(detached.join("agentd.lock").is_file());
  assert!(!run.join("agentd.lock").exists());
}

#[test]
fn lifecycle_lock_rejects_a_symlink() {
  let root = tempfile::tempdir().expect("temporary directory");
  let run = root.path().join("run");
  let outside = root.path().join("outside.lock");
  std::fs::write(&outside, []).expect("outside lock");
  let directory = SecureRunDirectory::open(&run).expect("run directory");
  symlink(&outside, run.join("agentd.lock")).expect("lock symlink");

  assert!(matches!(
    LifecycleLock::try_acquire(&directory),
    Err(DiscoveryError::SymlinkRejected(_))
  ));
}

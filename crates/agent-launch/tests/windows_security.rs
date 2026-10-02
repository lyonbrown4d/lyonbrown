#![cfg(windows)]

mod support;

use agent_launch::{DiscoverySource, LocalDiscoverySource};

#[test]
fn local_discovery_creates_a_secure_nested_windows_boundary() {
  let parent = tempfile::tempdir().expect("temporary root");
  let path = parent.path().join("missing").join("nested").join("run");
  let discovery = LocalDiscoverySource::new(&path);

  assert!(discovery.read().expect("empty discovery").is_none());
  let guard = discovery
    .try_acquire_startup_lock()
    .expect("startup lock attempt")
    .expect("startup lock");

  assert!(path.is_dir());
  drop(guard);
}

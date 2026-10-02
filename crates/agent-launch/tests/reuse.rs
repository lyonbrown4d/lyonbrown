mod support;

use agent_launch::{LaunchCoordinator, LaunchError, LaunchRequest};
use agent_local::ScopeFingerprint;
use std::sync::Arc;
use std::time::Duration;
use support::{FakeDiscovery, FakeProbe, FakeSpawner, descriptor, workspace};

fn request() -> LaunchRequest {
  LaunchRequest::new(
    workspace(),
    "runtime-config-v1",
    true,
    Duration::from_millis(200),
  )
  .expect("valid request")
}

#[tokio::test]
async fn reuses_authenticated_daemon_in_the_same_scope_without_spawning() {
  let request = request();
  let existing = descriptor(request.scope_fingerprint(), 41001);
  let source = FakeDiscovery::with_descriptor(existing.clone());
  let spawner = FakeSpawner::silent(source.clone());
  let probe = FakeProbe::default();
  probe.ready(&existing);
  let coordinator = LaunchCoordinator::new(
    Arc::new(source.clone()),
    Arc::new(spawner.clone()),
    Arc::new(probe),
  );

  let target = coordinator.connect_or_start(&request).await.unwrap();

  assert_eq!(
    target.descriptor().server_instance_id(),
    existing.server_instance_id()
  );
  assert_eq!(target.authenticated_target().endpoint(), target.endpoint());
  assert_eq!(spawner.spawn_count(), 0);
  assert_eq!(source.lock_attempts(), 0);
}

#[tokio::test]
async fn rejects_descriptor_from_another_scope() {
  let request = request();
  let other_scope = ScopeFingerprint::parse(&"f".repeat(64)).unwrap();
  let existing = descriptor(&other_scope, 41002);
  let source = FakeDiscovery::with_descriptor(existing.clone());
  let spawner = FakeSpawner::silent(source.clone());
  let probe = FakeProbe::default();
  probe.ready(&existing);
  let coordinator =
    LaunchCoordinator::new(Arc::new(source), Arc::new(spawner.clone()), Arc::new(probe));

  let error = coordinator.connect_or_start(&request).await.unwrap_err();

  assert!(matches!(error, LaunchError::ScopeMismatch { .. }));
  assert_eq!(spawner.spawn_count(), 0);
}

#[tokio::test]
async fn rejects_authentication_failure_without_replacing_daemon() {
  let request = request();
  let existing = descriptor(request.scope_fingerprint(), 41003);
  let source = FakeDiscovery::with_descriptor(existing.clone());
  let spawner = FakeSpawner::silent(source.clone());
  let probe = FakeProbe::default();
  probe.authentication_rejected(&existing);
  let coordinator =
    LaunchCoordinator::new(Arc::new(source), Arc::new(spawner.clone()), Arc::new(probe));

  let error = coordinator.connect_or_start(&request).await.unwrap_err();

  assert!(matches!(error, LaunchError::AuthenticationRejected));
  assert_eq!(spawner.spawn_count(), 0);
}

#[tokio::test]
async fn rejects_handshake_for_a_different_server_instance() {
  let request = request();
  let existing = descriptor(request.scope_fingerprint(), 41004);
  let source = FakeDiscovery::with_descriptor(existing.clone());
  let spawner = FakeSpawner::silent(source.clone());
  let probe = FakeProbe::default();
  probe.wrong_instance(&existing);
  let coordinator =
    LaunchCoordinator::new(Arc::new(source), Arc::new(spawner.clone()), Arc::new(probe));

  let error = coordinator.connect_or_start(&request).await.unwrap_err();

  assert!(matches!(error, LaunchError::HandshakeMismatch { .. }));
  assert_eq!(spawner.spawn_count(), 0);
}

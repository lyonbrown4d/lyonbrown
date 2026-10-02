mod support;

use agent_launch::{DaemonExit, LaunchCoordinator, LaunchError, LaunchRequest};
use agent_local::ScopeFingerprint;
use std::sync::Arc;
use std::time::Duration;
use support::{FakeDiscovery, FakeProbe, FakeSpawner, descriptor, workspace};

fn request(auto_start: bool, timeout: Duration) -> LaunchRequest {
  LaunchRequest::new(workspace(), "runtime-config-v1", auto_start, timeout).unwrap()
}

#[tokio::test]
async fn starts_missing_daemon_and_waits_for_authenticated_readiness() {
  let request = request(true, Duration::from_millis(200));
  let source = FakeDiscovery::default();
  let published = descriptor(request.scope_fingerprint(), 42001);
  let spawner = FakeSpawner::publishing(source.clone(), published.clone());
  let probe = FakeProbe::default();
  probe.ready(&published);
  let coordinator =
    LaunchCoordinator::new(Arc::new(source), Arc::new(spawner.clone()), Arc::new(probe));

  let target = coordinator.connect_or_start(&request).await.unwrap();

  assert_eq!(
    target.descriptor().server_instance_id(),
    published.server_instance_id()
  );
  assert_eq!(spawner.spawn_count(), 1);
  assert!(spawner.spawned_under_lock());
}

#[tokio::test]
async fn probes_stale_descriptor_again_under_lock_before_replacing_it() {
  let request = request(true, Duration::from_millis(200));
  let stale = descriptor(request.scope_fingerprint(), 42002);
  let source = FakeDiscovery::with_descriptor(stale.clone());
  let replacement = descriptor(request.scope_fingerprint(), 42003);
  let spawner = FakeSpawner::publishing(source.clone(), replacement.clone());
  let probe = FakeProbe::observing(source.clone());
  probe.unavailable(&stale);
  probe.ready(&replacement);
  let coordinator = LaunchCoordinator::new(
    Arc::new(source),
    Arc::new(spawner.clone()),
    Arc::new(probe.clone()),
  );

  coordinator.connect_or_start(&request).await.unwrap();

  let stale_observations: Vec<_> = probe
    .observations()
    .into_iter()
    .filter(|(id, _)| *id == stale.server_instance_id())
    .collect();
  assert_eq!(
    stale_observations,
    vec![
      (stale.server_instance_id(), false),
      (stale.server_instance_id(), true)
    ]
  );
  assert_eq!(spawner.spawn_count(), 1);
  assert!(spawner.spawned_under_lock());
}

#[tokio::test]
async fn replaces_an_unavailable_descriptor_from_an_old_runtime_scope() {
  let request = request(true, Duration::from_millis(200));
  let old_scope = ScopeFingerprint::parse(&"f".repeat(64)).unwrap();
  let stale = descriptor(&old_scope, 42004);
  let source = FakeDiscovery::with_descriptor(stale.clone());
  let replacement = descriptor(request.scope_fingerprint(), 42005);
  let spawner = FakeSpawner::publishing(source.clone(), replacement.clone());
  let probe = FakeProbe::observing(source.clone());
  probe.unavailable(&stale);
  probe.ready(&replacement);
  let coordinator =
    LaunchCoordinator::new(Arc::new(source), Arc::new(spawner.clone()), Arc::new(probe));

  let target = coordinator.connect_or_start(&request).await.unwrap();

  assert_eq!(
    target.descriptor().server_instance_id(),
    replacement.server_instance_id()
  );
  assert_eq!(spawner.spawn_count(), 1);
}

#[tokio::test]
async fn returns_timeout_when_spawned_daemon_never_publishes_readiness() {
  let request = request(true, Duration::from_millis(30));
  let source = FakeDiscovery::default();
  let spawner = FakeSpawner::silent(source.clone());
  let coordinator = LaunchCoordinator::new(
    Arc::new(source),
    Arc::new(spawner),
    Arc::new(FakeProbe::default()),
  )
  .with_poll_interval(Duration::from_millis(2));

  let error = coordinator.connect_or_start(&request).await.unwrap_err();

  assert!(matches!(error, LaunchError::ReadinessTimeout { .. }));
}

#[tokio::test]
async fn reports_daemon_exit_before_readiness_with_bounded_single_line_stderr() {
  let request = request(true, Duration::from_secs(1));
  let source = FakeDiscovery::default();
  let diagnostic = format!("invalid config\n{}", "x".repeat(8_000));
  let spawner = FakeSpawner::exiting(source.clone(), DaemonExit::new(Some(78), diagnostic));
  let coordinator = LaunchCoordinator::new(
    Arc::new(source),
    Arc::new(spawner),
    Arc::new(FakeProbe::default()),
  )
  .with_poll_interval(Duration::from_millis(2));

  let error = coordinator.connect_or_start(&request).await.unwrap_err();

  let LaunchError::DaemonExited { exit_code, stderr } = error else {
    panic!("expected daemon exit error, got {error:?}");
  };
  assert_eq!(exit_code, Some(78));
  assert!(stderr.starts_with("invalid config "));
  assert!(!stderr.contains('\r'));
  assert!(!stderr.contains('\n'));
  assert!(stderr.len() <= 1_024);
}

#[tokio::test]
async fn does_not_start_when_auto_start_is_disabled() {
  let request = request(false, Duration::from_millis(100));
  let source = FakeDiscovery::default();
  let spawner = FakeSpawner::silent(source.clone());
  let coordinator = LaunchCoordinator::new(
    Arc::new(source),
    Arc::new(spawner.clone()),
    Arc::new(FakeProbe::default()),
  );

  let error = coordinator.connect_or_start(&request).await.unwrap_err();

  assert!(matches!(error, LaunchError::AutoStartDisabled));
  assert_eq!(spawner.spawn_count(), 0);
}

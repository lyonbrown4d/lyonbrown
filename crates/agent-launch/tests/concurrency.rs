mod support;

use agent_launch::{DiscoverySource, LaunchCoordinator, LaunchRequest};
use std::sync::Arc;
use std::time::Duration;
use support::{FakeDiscovery, FakeProbe, FakeSpawner, descriptor, workspace};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_first_clients_spawn_exactly_one_daemon() {
  let request = Arc::new(
    LaunchRequest::new(
      workspace(),
      "runtime-config-v1",
      true,
      Duration::from_millis(300),
    )
    .unwrap(),
  );
  let source = FakeDiscovery::default();
  let published = descriptor(request.scope_fingerprint(), 43001);
  let spawner = FakeSpawner::publishing(source.clone(), published.clone());
  let probe = FakeProbe::default();
  probe.ready(&published);
  let coordinator = Arc::new(
    LaunchCoordinator::new(Arc::new(source), Arc::new(spawner.clone()), Arc::new(probe))
      .with_poll_interval(Duration::from_millis(1)),
  );

  let first = tokio::spawn({
    let coordinator = Arc::clone(&coordinator);
    let request = Arc::clone(&request);
    async move { coordinator.connect_or_start(&request).await }
  });
  let second = tokio::spawn({
    let coordinator = Arc::clone(&coordinator);
    let request = Arc::clone(&request);
    async move { coordinator.connect_or_start(&request).await }
  });

  let (first, second) = tokio::join!(first, second);
  assert_eq!(
    first.unwrap().unwrap().descriptor().server_instance_id(),
    published.server_instance_id()
  );
  assert_eq!(
    second.unwrap().unwrap().descriptor().server_instance_id(),
    published.server_instance_id()
  );
  assert_eq!(spawner.spawn_count(), 1);
}

#[tokio::test]
async fn dropping_target_does_not_own_or_stop_the_daemon_process() {
  let request = LaunchRequest::new(
    workspace(),
    "runtime-config-v1",
    true,
    Duration::from_millis(200),
  )
  .unwrap();
  let source = FakeDiscovery::default();
  let published = descriptor(request.scope_fingerprint(), 43002);
  let spawner = FakeSpawner::publishing(source.clone(), published.clone());
  let probe = FakeProbe::default();
  probe.ready(&published);
  let coordinator =
    LaunchCoordinator::new(Arc::new(source.clone()), Arc::new(spawner), Arc::new(probe));

  let target = coordinator.connect_or_start(&request).await.unwrap();
  drop(target);

  assert_eq!(
    source.read().unwrap().unwrap().server_instance_id(),
    published.server_instance_id()
  );
}

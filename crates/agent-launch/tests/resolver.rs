mod support;

use std::sync::Arc;
use std::time::Duration;

use agent_client::AuthenticatedTargetResolver;
use agent_launch::{CoordinatorTargetResolver, LaunchCoordinator, LaunchRequest};

use support::{FakeDiscovery, FakeProbe, FakeSpawner, descriptor, workspace};

#[tokio::test]
async fn resolver_uses_coordinator_validation_before_returning_target() {
  let request = LaunchRequest::new(
    workspace(),
    "runtime-config-v1",
    true,
    Duration::from_secs(1),
  )
  .unwrap();
  let descriptor = descriptor(request.scope_fingerprint(), 45123);
  let source = FakeDiscovery::with_descriptor(descriptor.clone());
  let probe = FakeProbe::default();
  probe.ready(&descriptor);
  let coordinator = Arc::new(LaunchCoordinator::new(
    Arc::new(source.clone()),
    Arc::new(FakeSpawner::silent(source)),
    Arc::new(probe),
  ));
  let resolver = CoordinatorTargetResolver::new(coordinator, request);

  let target = resolver.resolve().await.unwrap();

  assert_eq!(
    target.endpoint(),
    format!("http://{}", descriptor.address())
  );
}

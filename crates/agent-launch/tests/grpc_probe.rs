use agent_launch::{GrpcReadinessProbe, ReadinessProbe};
use std::sync::Arc;

#[test]
fn grpc_probe_implements_the_readiness_port() {
  let probe: Arc<dyn ReadinessProbe> = Arc::new(GrpcReadinessProbe::default());
  drop(probe);
}

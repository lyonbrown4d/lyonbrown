use std::sync::Arc;

use agent_kernel::Kernel;
use agent_model::ModelRegistry;
use provider_mock::{MockProvider, ScriptedStep};

#[tokio::test]
async fn next_turn_can_recover_from_provider_failure() {
  let provider = Arc::new(MockProvider::scripted(vec![
    ScriptedStep::error("temporary failure"),
    ScriptedStep::text("recovered"),
  ]));
  let registry = ModelRegistry::new();
  registry.register(provider);
  let mut kernel = Kernel::builder().providers(registry).build();

  assert!(kernel.run_turn("first").await.is_err());
  let second = kernel.run_turn("second").await.unwrap();
  assert_eq!(
    second.iter().find_map(|event| event.turn_answer()),
    Some("recovered")
  );
}

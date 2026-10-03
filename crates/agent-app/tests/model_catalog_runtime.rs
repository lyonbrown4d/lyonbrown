use agent_app::{
  ModelCatalogView, ModelPolicyView, ModelRuntimeView, ModelSelection, RuntimePhase,
  spawn_runtime_with_models,
};
use agent_kernel::Kernel;

#[tokio::test]
async fn runtime_initial_snapshot_atomically_carries_catalog_and_policy() {
  let model = ModelRuntimeView {
    catalog: Some(ModelCatalogView::default()),
    policy: ModelPolicyView {
      active: ModelSelection::Pinned {
        provider: "local".into(),
        model: "worker".into(),
      },
      pending: None,
    },
  };

  let (controller, runtime) = spawn_runtime_with_models(Kernel::builder().build(), model, 2);
  let snapshot = controller.snapshot();

  assert_eq!(snapshot.phase, RuntimePhase::Ready);
  assert_eq!(snapshot.model_catalog, Some(ModelCatalogView::default()));
  assert_eq!(
    snapshot.model_policy.active,
    ModelSelection::Pinned {
      provider: "local".into(),
      model: "worker".into(),
    }
  );
  drop(runtime);
}

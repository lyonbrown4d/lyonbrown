use agent_app::{
  ModelCatalogError, ModelCatalogView, ModelLocalityView, ModelOptionDisabledReason,
  ModelPolicyView, ModelSelection, ModelTierView, RuntimeSnapshot,
};
use agent_core::{Locality, ModelCapabilities, ModelTier};

fn capabilities(provider: &str, model: &str, tier: ModelTier) -> ModelCapabilities {
  ModelCapabilities {
    provider: provider.into(),
    model: model.into(),
    tier,
    locality: Locality::Local,
    context_window: 32_768,
    max_output_tokens: 4_096,
    supports_tools: true,
    supports_streaming: true,
    supports_json: true,
    supports_thinking: true,
    cost_per_mtok_in: 0.0,
    cost_per_mtok_out: 0.0,
  }
}

#[test]
fn catalog_projects_capabilities_and_worker_eligibility() {
  let mut control = capabilities("local", "router", ModelTier::Control);
  control.locality = Locality::Remote;
  control.supports_tools = false;
  control.supports_streaming = false;
  control.supports_json = false;
  control.supports_thinking = false;

  let catalog = ModelCatalogView::try_from_capabilities([
    control,
    capabilities("local", "worker", ModelTier::Worker),
  ])
  .unwrap();

  assert_eq!(catalog.models[0].route.provider, "local");
  assert_eq!(catalog.models[0].route.model, "router");
  assert_eq!(catalog.models[0].tier, ModelTierView::Control);
  assert_eq!(catalog.models[0].locality, ModelLocalityView::Remote);
  assert_eq!(catalog.models[0].context_window, 32_768);
  assert_eq!(catalog.models[0].max_output_tokens, 4_096);
  assert!(!catalog.models[0].supports_tools);
  assert!(!catalog.models[0].supports_streaming);
  assert!(!catalog.models[0].supports_json);
  assert!(!catalog.models[0].supports_thinking);
  assert!(!catalog.models[0].selectable_for_worker);
  assert_eq!(
    catalog.models[0].disabled_reason,
    Some(ModelOptionDisabledReason::NotWorkerTier)
  );
  assert_eq!(catalog.models[1].tier, ModelTierView::Worker);
  assert!(catalog.models[1].selectable_for_worker);
  assert_eq!(catalog.models[1].disabled_reason, None);
}

#[test]
fn worker_eligibility_reasons_are_computed_by_the_server_projection() {
  let mut without_tools = capabilities("local", "no-tools", ModelTier::Worker);
  without_tools.supports_tools = false;
  let mut without_streaming = capabilities("local", "no-stream", ModelTier::Worker);
  without_streaming.supports_streaming = false;

  let catalog =
    ModelCatalogView::try_from_capabilities([without_tools, without_streaming]).unwrap();

  assert_eq!(
    catalog.models[0].disabled_reason,
    Some(ModelOptionDisabledReason::StreamingUnsupported)
  );
  assert_eq!(
    catalog.models[1].disabled_reason,
    Some(ModelOptionDisabledReason::ToolsUnsupported)
  );
}

#[test]
fn catalog_has_stable_exact_route_order_at_the_limit() {
  let models = (0..128)
    .rev()
    .map(|index| capabilities("provider", &format!("model-{index:03}"), ModelTier::Worker))
    .collect::<Vec<_>>();

  let catalog = ModelCatalogView::try_from_capabilities(models).unwrap();

  assert_eq!(catalog.models.len(), 128);
  assert_eq!(catalog.models[0].route.model, "model-000");
  assert_eq!(catalog.models[127].route.model, "model-127");
}

#[test]
fn catalog_rejects_duplicate_exact_routes() {
  let error = ModelCatalogView::try_from_capabilities([
    capabilities("provider", "worker", ModelTier::Worker),
    capabilities("provider", "worker", ModelTier::Worker),
  ])
  .unwrap_err();

  assert_eq!(error, ModelCatalogError::DuplicateRoute);
}

#[test]
fn catalog_rejects_more_than_the_limit() {
  let error = ModelCatalogView::try_from_capabilities(
    (0..129).map(|index| capabilities("provider", &format!("model-{index:03}"), ModelTier::Worker)),
  )
  .unwrap_err();

  assert_eq!(error, ModelCatalogError::TooManyModels);
}

#[test]
fn catalog_enforces_provider_and_model_length_boundaries() {
  let accepted = ModelCatalogView::try_from_capabilities([capabilities(
    &"p".repeat(128),
    &"m".repeat(256),
    ModelTier::Worker,
  )]);
  assert!(accepted.is_ok());

  for (provider, model) in [
    ("", "worker"),
    ("local", ""),
    (&"p".repeat(129), "worker"),
    ("local", &"m".repeat(257)),
  ] {
    let error =
      ModelCatalogView::try_from_capabilities([capabilities(provider, model, ModelTier::Worker)])
        .unwrap_err();
    assert!(matches!(error, ModelCatalogError::InvalidRoute));
  }
}

#[test]
fn default_snapshot_has_no_catalog_and_auto_active_policy() {
  let snapshot = RuntimeSnapshot::default();

  assert_eq!(snapshot.model_catalog, None);
  assert_eq!(
    snapshot.model_policy,
    ModelPolicyView {
      active: ModelSelection::Auto,
      pending: None,
    }
  );
}

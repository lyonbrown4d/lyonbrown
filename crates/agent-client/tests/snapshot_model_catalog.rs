use agent_app::{
  ModelCatalogView, ModelLocalityView, ModelOptionDisabledReason, ModelPolicyView, ModelSelection,
  ModelTierView,
};
use agent_client::{SnapshotDecodeError, decode_snapshot};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::model_selection::Selection;
use agent_protocol::v1::{
  ModelCatalog, ModelLocality, ModelOption, ModelOptionDisabledReason as WireDisabledReason,
  ModelPolicy, ModelRoute, ModelSelection as WireSelection, ModelTier, RuntimePhase,
  RuntimeSnapshot,
};

fn selection(selection: Selection) -> WireSelection {
  WireSelection {
    selection: Some(selection),
  }
}

fn base_snapshot() -> RuntimeSnapshot {
  RuntimeSnapshot {
    protocol_version: PROTOCOL_VERSION,
    revision: 7,
    phase: RuntimePhase::Ready as i32,
    model_selection: Some(selection(Selection::Auto(true))),
    ..RuntimeSnapshot::default()
  }
}

fn model(provider: &str, name: &str) -> ModelOption {
  ModelOption {
    route: Some(ModelRoute {
      provider: provider.into(),
      model: name.into(),
    }),
    tier: ModelTier::Worker as i32,
    locality: ModelLocality::Local as i32,
    context_window: 32_768,
    max_output_tokens: 4_096,
    supports_tools: true,
    supports_streaming: true,
    supports_json: true,
    supports_thinking: true,
    selectable_for_worker: true,
    disabled_reason: None,
  }
}

#[test]
fn absent_and_present_empty_catalog_are_distinct() {
  let absent = decode_snapshot(base_snapshot()).unwrap().snapshot;
  assert_eq!(absent.model_catalog, None);

  let mut wire = base_snapshot();
  wire.model_catalog = Some(ModelCatalog { models: Vec::new() });
  let empty = decode_snapshot(wire).unwrap().snapshot;
  assert_eq!(empty.model_catalog, Some(ModelCatalogView::default()));
}

#[test]
fn catalog_and_active_pending_policy_decode_all_fields() {
  let mut remote = model("remote", "frontier");
  remote.tier = ModelTier::Frontier as i32;
  remote.locality = ModelLocality::Remote as i32;
  remote.supports_tools = false;
  remote.supports_streaming = false;
  remote.supports_json = false;
  remote.supports_thinking = false;
  remote.selectable_for_worker = false;
  remote.disabled_reason = Some(WireDisabledReason::NotWorkerTier as i32);
  let mut wire = base_snapshot();
  wire.model_catalog = Some(ModelCatalog {
    models: vec![remote],
  });
  wire.model_selection = Some(selection(Selection::Pinned(ModelRoute {
    provider: "local".into(),
    model: "worker".into(),
  })));
  wire.model_policy = Some(ModelPolicy {
    active: Some(selection(Selection::Pinned(ModelRoute {
      provider: "local".into(),
      model: "worker".into(),
    }))),
    pending: Some(selection(Selection::Auto(true))),
  });

  let decoded = decode_snapshot(wire).unwrap().snapshot;
  let option = &decoded.model_catalog.unwrap().models[0];
  assert_eq!(option.tier, ModelTierView::Frontier);
  assert_eq!(option.locality, ModelLocalityView::Remote);
  assert_eq!(option.context_window, 32_768);
  assert_eq!(option.max_output_tokens, 4_096);
  assert!(!option.supports_tools);
  assert!(!option.supports_streaming);
  assert!(!option.supports_json);
  assert!(!option.supports_thinking);
  assert!(!option.selectable_for_worker);
  assert_eq!(
    option.disabled_reason,
    Some(ModelOptionDisabledReason::NotWorkerTier)
  );
  assert_eq!(
    decoded.model_policy,
    ModelPolicyView {
      active: ModelSelection::Pinned {
        provider: "local".into(),
        model: "worker".into(),
      },
      pending: Some(ModelSelection::Auto),
    }
  );
}

#[test]
fn missing_new_policy_falls_back_to_legacy_active_selection() {
  let decoded = decode_snapshot(base_snapshot()).unwrap().snapshot;

  assert_eq!(decoded.model_policy, ModelPolicyView::default());
}

#[test]
fn conflicting_legacy_and_new_active_policy_is_rejected() {
  let mut wire = base_snapshot();
  wire.model_policy = Some(ModelPolicy {
    active: Some(selection(Selection::Pinned(ModelRoute {
      provider: "local".into(),
      model: "worker".into(),
    }))),
    pending: None,
  });

  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelPolicy(_))
  ));
}

#[test]
fn invalid_catalog_wire_values_fail_closed() {
  let mut cases = Vec::new();
  let mut unknown_tier = model("local", "worker");
  unknown_tier.tier = 999;
  cases.push(unknown_tier);
  let mut unknown_locality = model("local", "worker");
  unknown_locality.locality = 999;
  cases.push(unknown_locality);
  let mut unknown_reason = model("local", "worker");
  unknown_reason.disabled_reason = Some(999);
  cases.push(unknown_reason);
  cases.push(model("", "worker"));
  cases.push(model("local", &"m".repeat(257)));

  for invalid in cases {
    let mut wire = base_snapshot();
    wire.model_catalog = Some(ModelCatalog {
      models: vec![invalid],
    });
    assert!(decode_snapshot(wire).is_err());
  }
}

#[test]
fn shared_route_length_limits_are_accepted_on_the_wire() {
  let mut wire = base_snapshot();
  wire.model_catalog = Some(ModelCatalog {
    models: vec![model(&"p".repeat(128), &"m".repeat(256))],
  });

  let decoded = decode_snapshot(wire).unwrap().snapshot;
  let route = &decoded.model_catalog.unwrap().models[0].route;
  assert_eq!(route.provider.len(), 128);
  assert_eq!(route.model.len(), 256);
}

#[test]
fn duplicate_and_oversized_catalogs_fail_closed() {
  let mut duplicate = base_snapshot();
  duplicate.model_catalog = Some(ModelCatalog {
    models: vec![model("local", "worker"), model("local", "worker")],
  });
  assert!(matches!(
    decode_snapshot(duplicate),
    Err(SnapshotDecodeError::InvalidModelCatalog(_))
  ));

  let mut oversized = base_snapshot();
  oversized.model_catalog = Some(ModelCatalog {
    models: (0..129)
      .map(|index| model("local", &format!("worker-{index}")))
      .collect(),
  });
  assert!(matches!(
    decode_snapshot(oversized),
    Err(SnapshotDecodeError::InvalidModelCatalog(_))
  ));
}

#[test]
fn inconsistent_selectability_is_rejected() {
  let mut inconsistent = model("local", "worker");
  inconsistent.selectable_for_worker = false;
  let mut wire = base_snapshot();
  wire.model_catalog = Some(ModelCatalog {
    models: vec![inconsistent],
  });

  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelCatalog(_))
  ));
}

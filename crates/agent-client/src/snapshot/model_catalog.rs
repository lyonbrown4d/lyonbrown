use std::collections::HashSet;

use agent_app::{
  MAX_MODEL_CATALOG_SIZE, MAX_MODEL_ID_BYTES, MAX_MODEL_PROVIDER_BYTES, ModelCatalogView,
  ModelLocalityView, ModelOptionDisabledReason, ModelOptionView, ModelPolicyView, ModelRoute,
  ModelTierView,
};
use agent_protocol::v1::{ModelLocality, ModelTier};

use super::{SnapshotDecodeError, model_selection_from_wire};

pub(super) fn catalog_from_wire(
  wire: agent_protocol::v1::ModelCatalog,
) -> Result<ModelCatalogView, SnapshotDecodeError> {
  if wire.models.len() > MAX_MODEL_CATALOG_SIZE {
    return Err(SnapshotDecodeError::InvalidModelCatalog("too many models"));
  }
  let mut routes = HashSet::with_capacity(wire.models.len());
  let mut models = Vec::with_capacity(wire.models.len());
  for model in wire.models {
    let option = option_from_wire(model)?;
    if !routes.insert((option.route.provider.clone(), option.route.model.clone())) {
      return Err(SnapshotDecodeError::InvalidModelCatalog("duplicate route"));
    }
    models.push(option);
  }
  models.sort_by(|left, right| {
    (&left.route.provider, &left.route.model).cmp(&(&right.route.provider, &right.route.model))
  });
  Ok(ModelCatalogView { models })
}

pub(super) fn policy_from_wire(
  wire: agent_protocol::v1::ModelPolicy,
) -> Result<ModelPolicyView, SnapshotDecodeError> {
  Ok(ModelPolicyView {
    active: model_selection_from_wire(
      wire
        .active
        .ok_or(SnapshotDecodeError::Missing("model_policy.active"))?,
    )?,
    pending: wire.pending.map(model_selection_from_wire).transpose()?,
  })
}

fn option_from_wire(
  wire: agent_protocol::v1::ModelOption,
) -> Result<ModelOptionView, SnapshotDecodeError> {
  let route = wire
    .route
    .ok_or(SnapshotDecodeError::Missing("model_catalog.models.route"))?;
  validate_route_part(&route.provider, MAX_MODEL_PROVIDER_BYTES)?;
  validate_route_part(&route.model, MAX_MODEL_ID_BYTES)?;
  let tier = tier_from_wire(wire.tier)?;
  let locality = locality_from_wire(wire.locality)?;
  let disabled_reason = wire
    .disabled_reason
    .map(disabled_reason_from_wire)
    .transpose()?;
  if wire.selectable_for_worker == disabled_reason.is_some() {
    return Err(SnapshotDecodeError::InvalidModelCatalog(
      "inconsistent worker selectability",
    ));
  }
  Ok(ModelOptionView {
    route: ModelRoute {
      provider: route.provider,
      model: route.model,
    },
    tier,
    locality,
    context_window: wire.context_window,
    max_output_tokens: wire.max_output_tokens,
    supports_tools: wire.supports_tools,
    supports_streaming: wire.supports_streaming,
    supports_json: wire.supports_json,
    supports_thinking: wire.supports_thinking,
    selectable_for_worker: wire.selectable_for_worker,
    disabled_reason,
  })
}

fn validate_route_part(value: &str, max_bytes: usize) -> Result<(), SnapshotDecodeError> {
  if value.trim().is_empty() || value.len() > max_bytes {
    Err(SnapshotDecodeError::InvalidModelCatalog("invalid route"))
  } else {
    Ok(())
  }
}

fn tier_from_wire(value: i32) -> Result<ModelTierView, SnapshotDecodeError> {
  match ModelTier::try_from(value) {
    Ok(ModelTier::Control) => Ok(ModelTierView::Control),
    Ok(ModelTier::Worker) => Ok(ModelTierView::Worker),
    Ok(ModelTier::Frontier) => Ok(ModelTierView::Frontier),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "model_catalog.models.tier",
      value,
    }),
  }
}

fn locality_from_wire(value: i32) -> Result<ModelLocalityView, SnapshotDecodeError> {
  match ModelLocality::try_from(value) {
    Ok(ModelLocality::Local) => Ok(ModelLocalityView::Local),
    Ok(ModelLocality::Remote) => Ok(ModelLocalityView::Remote),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "model_catalog.models.locality",
      value,
    }),
  }
}

fn disabled_reason_from_wire(value: i32) -> Result<ModelOptionDisabledReason, SnapshotDecodeError> {
  use agent_protocol::v1::ModelOptionDisabledReason as WireReason;
  match WireReason::try_from(value) {
    Ok(WireReason::NotWorkerTier) => Ok(ModelOptionDisabledReason::NotWorkerTier),
    Ok(WireReason::ToolsUnsupported) => Ok(ModelOptionDisabledReason::ToolsUnsupported),
    Ok(WireReason::StreamingUnsupported) => Ok(ModelOptionDisabledReason::StreamingUnsupported),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "model_catalog.models.disabled_reason",
      value,
    }),
  }
}

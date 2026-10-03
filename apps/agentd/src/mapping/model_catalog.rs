use agent_app::{
  ModelCatalogView, ModelLocalityView, ModelOptionDisabledReason, ModelOptionView, ModelPolicyView,
  ModelTierView,
};
use agent_protocol::v1::{
  ModelCatalog, ModelLocality, ModelOption, ModelOptionDisabledReason as WireDisabledReason,
  ModelPolicy, ModelTier,
};

use super::model_selection_to_wire;
use crate::mapping::common::model_route_to_wire;

pub(super) fn model_catalog_to_wire(catalog: ModelCatalogView) -> ModelCatalog {
  ModelCatalog {
    models: catalog
      .models
      .into_iter()
      .map(model_option_to_wire)
      .collect(),
  }
}

pub(super) fn model_policy_to_wire(policy: ModelPolicyView) -> ModelPolicy {
  ModelPolicy {
    active: Some(model_selection_to_wire(policy.active)),
    pending: policy.pending.map(model_selection_to_wire),
  }
}

fn model_option_to_wire(option: ModelOptionView) -> ModelOption {
  ModelOption {
    route: Some(model_route_to_wire(option.route)),
    tier: tier_to_wire(option.tier) as i32,
    locality: locality_to_wire(option.locality) as i32,
    context_window: option.context_window,
    max_output_tokens: option.max_output_tokens,
    supports_tools: option.supports_tools,
    supports_streaming: option.supports_streaming,
    supports_json: option.supports_json,
    supports_thinking: option.supports_thinking,
    selectable_for_worker: option.selectable_for_worker,
    disabled_reason: option
      .disabled_reason
      .map(|reason| reason_to_wire(reason) as i32),
  }
}

fn tier_to_wire(tier: ModelTierView) -> ModelTier {
  match tier {
    ModelTierView::Control => ModelTier::Control,
    ModelTierView::Worker => ModelTier::Worker,
    ModelTierView::Frontier => ModelTier::Frontier,
  }
}

fn locality_to_wire(locality: ModelLocalityView) -> ModelLocality {
  match locality {
    ModelLocalityView::Local => ModelLocality::Local,
    ModelLocalityView::Remote => ModelLocality::Remote,
  }
}

fn reason_to_wire(reason: ModelOptionDisabledReason) -> WireDisabledReason {
  match reason {
    ModelOptionDisabledReason::NotWorkerTier => WireDisabledReason::NotWorkerTier,
    ModelOptionDisabledReason::ToolsUnsupported => WireDisabledReason::ToolsUnsupported,
    ModelOptionDisabledReason::StreamingUnsupported => WireDisabledReason::StreamingUnsupported,
  }
}

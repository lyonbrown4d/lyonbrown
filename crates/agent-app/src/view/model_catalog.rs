use agent_core::{Locality, ModelCapabilities, ModelTier};

pub use agent_core::{MAX_MODEL_CATALOG_SIZE, MAX_MODEL_ID_BYTES, MAX_MODEL_PROVIDER_BYTES};

use super::{ModelRoute, ModelSelection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTierView {
  Control,
  Worker,
  Frontier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLocalityView {
  Local,
  Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOptionDisabledReason {
  NotWorkerTier,
  ToolsUnsupported,
  StreamingUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOptionView {
  pub route: ModelRoute,
  pub tier: ModelTierView,
  pub locality: ModelLocalityView,
  pub context_window: u32,
  pub max_output_tokens: u32,
  pub supports_tools: bool,
  pub supports_streaming: bool,
  pub supports_json: bool,
  pub supports_thinking: bool,
  pub selectable_for_worker: bool,
  pub disabled_reason: Option<ModelOptionDisabledReason>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelCatalogView {
  pub models: Vec<ModelOptionView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPolicyView {
  pub active: ModelSelection,
  pub pending: Option<ModelSelection>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelRuntimeView {
  pub catalog: Option<ModelCatalogView>,
  pub policy: ModelPolicyView,
}

impl Default for ModelPolicyView {
  fn default() -> Self {
    Self {
      active: ModelSelection::Auto,
      pending: None,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ModelCatalogError {
  #[error("model catalog contains an invalid route")]
  InvalidRoute,
  #[error("model catalog contains a duplicate exact route")]
  DuplicateRoute,
  #[error("model catalog exceeds the model limit")]
  TooManyModels,
}

impl ModelCatalogView {
  pub fn try_from_capabilities(
    capabilities: impl IntoIterator<Item = ModelCapabilities>,
  ) -> Result<Self, ModelCatalogError> {
    let capabilities = capabilities.into_iter().collect::<Vec<_>>();
    if capabilities.len() > MAX_MODEL_CATALOG_SIZE {
      return Err(ModelCatalogError::TooManyModels);
    }
    let mut models = capabilities
      .into_iter()
      .map(ModelOptionView::try_from)
      .collect::<Result<Vec<_>, _>>()?;
    models.sort_by(|left, right| {
      (&left.route.provider, &left.route.model).cmp(&(&right.route.provider, &right.route.model))
    });
    if models.windows(2).any(|pair| pair[0].route == pair[1].route) {
      return Err(ModelCatalogError::DuplicateRoute);
    }
    Ok(Self { models })
  }
}

impl TryFrom<ModelCapabilities> for ModelOptionView {
  type Error = ModelCatalogError;

  fn try_from(capabilities: ModelCapabilities) -> Result<Self, Self::Error> {
    validate_route_part(&capabilities.provider, MAX_MODEL_PROVIDER_BYTES)?;
    validate_route_part(&capabilities.model, MAX_MODEL_ID_BYTES)?;
    let disabled_reason = worker_disabled_reason(&capabilities);
    Ok(Self {
      route: ModelRoute {
        provider: capabilities.provider,
        model: capabilities.model,
      },
      tier: tier_view(capabilities.tier),
      locality: locality_view(capabilities.locality),
      context_window: capabilities.context_window,
      max_output_tokens: capabilities.max_output_tokens,
      supports_tools: capabilities.supports_tools,
      supports_streaming: capabilities.supports_streaming,
      supports_json: capabilities.supports_json,
      supports_thinking: capabilities.supports_thinking,
      selectable_for_worker: disabled_reason.is_none(),
      disabled_reason,
    })
  }
}

fn validate_route_part(value: &str, max_bytes: usize) -> Result<(), ModelCatalogError> {
  if value.trim().is_empty() || value.len() > max_bytes {
    Err(ModelCatalogError::InvalidRoute)
  } else {
    Ok(())
  }
}

fn worker_disabled_reason(capabilities: &ModelCapabilities) -> Option<ModelOptionDisabledReason> {
  if capabilities.tier != ModelTier::Worker {
    Some(ModelOptionDisabledReason::NotWorkerTier)
  } else if !capabilities.supports_tools {
    Some(ModelOptionDisabledReason::ToolsUnsupported)
  } else if !capabilities.supports_streaming {
    Some(ModelOptionDisabledReason::StreamingUnsupported)
  } else {
    None
  }
}

fn tier_view(tier: ModelTier) -> ModelTierView {
  match tier {
    ModelTier::Control => ModelTierView::Control,
    ModelTier::Worker => ModelTierView::Worker,
    ModelTier::Frontier => ModelTierView::Frontier,
  }
}

fn locality_view(locality: Locality) -> ModelLocalityView {
  match locality {
    Locality::Local => ModelLocalityView::Local,
    Locality::Remote => ModelLocalityView::Remote,
  }
}

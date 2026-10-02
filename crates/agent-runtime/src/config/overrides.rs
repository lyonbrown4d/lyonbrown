use super::CliOverrides;

pub(super) fn apply_cli_overrides(
  mut builder: config::ConfigBuilder<config::builder::DefaultState>,
  overrides: CliOverrides,
) -> Result<config::ConfigBuilder<config::builder::DefaultState>, config::ConfigError> {
  if let Some(value) = overrides.profile {
    builder = builder.set_override("profile", value)?;
  }
  if let Some(value) = overrides.provider {
    builder = builder.set_override("model.provider", value)?;
  }
  if let Some(value) = overrides.model {
    builder = builder.set_override("model.model", value)?;
  }
  if let Some(value) = overrides.endpoint {
    builder = builder.set_override("ollama.endpoint", value)?;
  }
  if let Some(value) = overrides.max_tokens {
    builder = builder.set_override("model.max_tokens", value)?;
  }
  if let Some(value) = overrides.temperature {
    builder = builder.set_override("model.temperature", f64::from(value))?;
  }
  if let Some(value) = overrides.approve_all {
    builder = builder.set_override("approval.approve_all", value)?;
  }
  if let Some(value) = overrides.deny_all {
    builder = builder.set_override("approval.deny_all", value)?;
  }
  Ok(builder)
}

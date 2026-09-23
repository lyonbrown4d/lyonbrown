use std::io::ErrorKind;
use std::path::Path;

use config::{Config, File, FileFormat};

use super::ConfigLoadError;

pub(super) fn checked_source(
  path: &Path,
  trust_workspace_mcp: bool,
  trust_workspace_config: bool,
) -> Result<Option<File<config::FileSourceString, FileFormat>>, ConfigLoadError> {
  let content = match std::fs::read_to_string(path) {
    Ok(content) => content,
    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
    Err(error) => {
      return Err(ConfigLoadError::Validation(format!(
        "cannot read workspace configuration: {error}"
      )));
    }
  };
  let source = File::from_str(&content, FileFormat::Toml);
  let workspace_only = Config::builder().add_source(source.clone()).build()?;
  let defines_mcp_servers = workspace_only
    .get_table("mcp.servers")
    .is_ok_and(|servers| !servers.is_empty());
  if defines_mcp_servers && !trust_workspace_mcp {
    return Err(ConfigLoadError::Validation(
      "workspace MCP commands require --trust-workspace-mcp".into(),
    ));
  }
  let selects_non_default_provider = workspace_only
    .get_string("model.provider")
    .is_ok_and(|provider| provider != "ollama");
  let configures_model_or_remote_provider = workspace_only.get_string("ollama.endpoint").is_ok()
    || workspace_only.get_table("llamacpp").is_ok()
    || workspace_only.get_table("anthropic").is_ok()
    || workspace_only.get_table("openai").is_ok();
  let overrides_profile = workspace_only.get_string("profile").is_ok();
  let configures_approval_or_storage =
    workspace_only.get_table("approval").is_ok() || workspace_only.get_table("storage").is_ok();
  if (selects_non_default_provider
    || configures_model_or_remote_provider
    || overrides_profile
    || configures_approval_or_storage)
    && !trust_workspace_config
  {
    return Err(ConfigLoadError::Validation(
      "workspace profile, model, approval, and storage settings require --trust-workspace-config"
        .into(),
    ));
  }
  Ok(Some(source))
}

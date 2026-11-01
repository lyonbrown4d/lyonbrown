//! Provider metadata derived from the daemon's effective layered configuration.
//! Detection never performs network requests or grants model access.
use std::collections::BTreeMap;

use serde::Serialize;

use crate::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
  OpenaiResponses,
  OpenaiCompletions,
  AnthropicMessages,
  Local,
}

/// A catalog entry describes integration support, not a reachable service.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ProviderDefinition {
  pub id: &'static str,
  pub name: &'static str,
  pub aliases: &'static [&'static str],
  pub credential_env_names: &'static [&'static str],
  pub protocols: &'static [ProviderProtocol],
  pub runtime_supported: bool,
}

pub const PROVIDER_CATALOG: &[ProviderDefinition] = &[
  ProviderDefinition {
    id: "openai",
    name: "OpenAI / compatible endpoint",
    aliases: &["openai-responses", "openai-completions"],
    credential_env_names: &["OPENAI_API_KEY"],
    protocols: &[
      ProviderProtocol::OpenaiResponses,
      ProviderProtocol::OpenaiCompletions,
    ],
    runtime_supported: true,
  },
  ProviderDefinition {
    id: "anthropic",
    name: "Anthropic",
    aliases: &[],
    credential_env_names: &["ANTHROPIC_API_KEY"],
    protocols: &[ProviderProtocol::AnthropicMessages],
    runtime_supported: true,
  },
  ProviderDefinition {
    id: "local",
    name: "Local inference",
    aliases: &[],
    credential_env_names: &[],
    protocols: &[ProviderProtocol::Local],
    runtime_supported: true,
  },
  ProviderDefinition {
    id: "openrouter",
    name: "OpenRouter",
    aliases: &[],
    credential_env_names: &["OPENROUTER_API_KEY"],
    protocols: &[],
    runtime_supported: false,
  },
  ProviderDefinition {
    id: "gemini",
    name: "Google Gemini",
    aliases: &[],
    credential_env_names: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
    protocols: &[],
    runtime_supported: false,
  },
  ProviderDefinition {
    id: "deepseek",
    name: "DeepSeek",
    aliases: &[],
    credential_env_names: &["DEEPSEEK_API_KEY"],
    protocols: &[],
    runtime_supported: false,
  },
  ProviderDefinition {
    id: "mistral",
    name: "Mistral API",
    aliases: &[],
    credential_env_names: &["MISTRAL_API_KEY"],
    protocols: &[],
    runtime_supported: false,
  },
  ProviderDefinition {
    id: "groq",
    name: "Groq",
    aliases: &[],
    credential_env_names: &["GROQ_API_KEY"],
    protocols: &[],
    runtime_supported: false,
  },
];

pub fn provider_definition(id: &str) -> Option<&'static ProviderDefinition> {
  PROVIDER_CATALOG
    .iter()
    .find(|entry| entry.id == id || entry.aliases.contains(&id))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
  DaemonEnvironment,
  ClientEnvironment,
  Dotenv,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
  #[default]
  Missing,
  Detected,
  Invalid,
  NotRequired,
}

/// Safe to expose over the authenticated control protocol. Contains no value,
/// hash, prefix, suffix or length derived from a credential.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProviderCredentialStatus {
  pub state: CredentialState,
  pub source: Option<CredentialSource>,
  pub environment_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfiguredProviderModel {
  pub id: String,
  pub selected: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderStatus {
  pub definition: ProviderDefinition,
  pub selected: bool,
  pub credential: ProviderCredentialStatus,
  /// Configured model IDs only; successful loading/reachability is determined
  /// by agentd, and must not be inferred from this list.
  pub configured_models: Vec<ConfiguredProviderModel>,
}

impl Settings {
  /// Snapshot from this process's effective configuration. Client environment
  /// hints are deliberately not accepted by this API.
  pub fn provider_inventory(&self) -> Vec<ProviderStatus> {
    PROVIDER_CATALOG
      .iter()
      .map(|definition| {
        let selected =
          definition.id == self.provider || definition.aliases.contains(&self.provider.as_str());
        let configured_models = if definition.id == "local" {
          self
            .local_models
            .iter()
            .map(|model| ConfiguredProviderModel {
              id: model.id.clone(),
              selected: selected && self.model == model.id,
            })
            .collect()
        } else {
          selected
            .then(|| ConfiguredProviderModel {
              id: self.model.clone(),
              selected: true,
            })
            .into_iter()
            .collect()
        };
        ProviderStatus {
          definition: *definition,
          selected,
          credential: self
            .provider_credentials
            .get(definition.id)
            .cloned()
            .unwrap_or_default(),
          configured_models,
        }
      })
      .collect()
  }
}

/// Inspect only catalog credential names in the current process. The returned
/// metadata can be used as a client capability hint; it is not evidence that a
/// remote daemon owns the same credential or may use the provider.
pub fn detect_provider_environment() -> BTreeMap<String, ProviderCredentialStatus> {
  let environment = PROVIDER_CATALOG
    .iter()
    .flat_map(|entry| entry.credential_env_names)
    .filter_map(|name| match std::env::var(name) {
      Ok(value) => Some(((*name).into(), value)),
      Err(std::env::VarError::NotPresent) => None,
      // Retain only an invalid marker; never serialize or coerce a non-UTF8 value.
      Err(std::env::VarError::NotUnicode(_)) => Some(((*name).into(), "\0".into())),
    })
    .collect();
  let mut statuses = detect_provider_credentials(&BTreeMap::new(), &environment);
  for status in statuses.values_mut() {
    if status.source == Some(CredentialSource::DaemonEnvironment) {
      status.source = Some(CredentialSource::ClientEnvironment);
    }
  }
  statuses
}

/// Derive redacted metadata from explicit environment snapshots. The caller
/// owns the snapshots; none of their credential values is retained here.
pub fn detect_provider_credentials(
  dotenv: &BTreeMap<String, String>,
  environment: &BTreeMap<String, String>,
) -> BTreeMap<String, ProviderCredentialStatus> {
  PROVIDER_CATALOG
    .iter()
    .map(|entry| {
      let status = if entry.credential_env_names.is_empty() {
        ProviderCredentialStatus {
          state: CredentialState::NotRequired,
          ..ProviderCredentialStatus::default()
        }
      } else {
        entry
          .credential_env_names
          .iter()
          .find_map(|name| {
            let (value, source) = environment
              .get(*name)
              .map(|value| (value, CredentialSource::DaemonEnvironment))
              .or_else(|| {
                dotenv
                  .get(*name)
                  .map(|value| (value, CredentialSource::Dotenv))
              })?;
            Some(ProviderCredentialStatus {
              state: if value.trim().is_empty() {
                CredentialState::Missing
              } else if value.contains('\0') {
                CredentialState::Invalid
              } else {
                CredentialState::Detected
              },
              source: Some(source),
              environment_name: Some((*name).into()),
            })
          })
          .unwrap_or_default()
      };
      (entry.id.into(), status)
    })
    .collect()
}

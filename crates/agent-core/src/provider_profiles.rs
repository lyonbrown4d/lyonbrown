//! Daemon-owned remote provider configuration, without credential values or SDK types.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteProviderProtocol {
  OpenaiResponses,
  OpenaiCompletions,
  AnthropicMessages,
}
impl RemoteProviderProtocol {
  pub fn runtime_provider(self) -> &'static str {
    match self {
      Self::OpenaiResponses => "openai-responses",
      Self::OpenaiCompletions => "openai-completions",
      Self::AnthropicMessages => "anthropic",
    }
  }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderProfile {
  pub id: String,
  pub name: String,
  pub protocol: RemoteProviderProtocol,
  pub base_url: String,
  pub model: String,
  pub revision: u64,
  /// Immutable secret reference. Never a credential, and not returned in public DTOs.
  pub credential_id: String,
}

//! Daemon-owned remote provider configuration, without credential values or SDK types.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteProviderProtocol {
  OpenaiResponses,
  OpenaiCompletions,
  AnthropicMessages,
  GeminiGenerateContent,
}
impl RemoteProviderProtocol {
  pub fn runtime_provider(self) -> &'static str {
    match self {
      Self::OpenaiResponses => "openai-responses",
      Self::OpenaiCompletions => "openai-completions",
      Self::AnthropicMessages => "anthropic",
      Self::GeminiGenerateContent => "gemini",
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

impl ProviderProfile {
  /// Preserve native vendor semantics at the product's known public endpoints.
  /// Custom gateways retain the explicitly selected protocol family.
  pub fn runtime_provider(&self) -> &'static str {
    if self.protocol == RemoteProviderProtocol::OpenaiCompletions {
      match self.base_url.trim_end_matches('/') {
        "https://openrouter.ai/api/v1" => return "openrouter",
        "https://api.groq.com/openai/v1" => return "groq",
        "https://api.mistral.ai/v1" => return "mistral",
        "https://api.deepseek.com" | "https://api.deepseek.com/v1" => return "deepseek",
        _ => (),
      }
    }
    self.protocol.runtime_provider()
  }
}

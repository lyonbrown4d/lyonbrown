//! Product-owned defaults for explicit configuration; these do not prove model availability.
use super::ProviderProtocol;
#[derive(Clone, Copy, Debug)]
pub struct ProviderPreset {
  pub name: &'static str,
  pub protocol: ProviderProtocol,
  pub base_url: &'static str,
  pub model: &'static str,
}
pub const REMOTE_PROVIDER_PRESETS: &[ProviderPreset] = &[
  ProviderPreset {
    name: "OpenAI Responses",
    protocol: ProviderProtocol::OpenaiResponses,
    base_url: "https://api.openai.com/v1",
    model: "gpt-4.1",
  },
  ProviderPreset {
    name: "OpenAI Chat Completions",
    protocol: ProviderProtocol::OpenaiCompletions,
    base_url: "https://api.openai.com/v1",
    model: "gpt-4.1",
  },
  ProviderPreset {
    name: "Anthropic",
    protocol: ProviderProtocol::AnthropicMessages,
    base_url: "https://api.anthropic.com",
    model: "claude-sonnet-4-5",
  },
  ProviderPreset {
    name: "Google Gemini",
    protocol: ProviderProtocol::GeminiGenerateContent,
    base_url: "https://generativelanguage.googleapis.com",
    model: "gemini-2.5-flash",
  },
  ProviderPreset {
    name: "OpenRouter",
    protocol: ProviderProtocol::OpenaiCompletions,
    base_url: "https://openrouter.ai/api/v1",
    model: "openrouter/auto",
  },
  ProviderPreset {
    name: "Groq",
    protocol: ProviderProtocol::OpenaiCompletions,
    base_url: "https://api.groq.com/openai/v1",
    model: "llama-3.3-70b-versatile",
  },
  ProviderPreset {
    name: "Mistral API",
    protocol: ProviderProtocol::OpenaiCompletions,
    base_url: "https://api.mistral.ai/v1",
    model: "mistral-small-latest",
  },
  ProviderPreset {
    name: "DeepSeek",
    protocol: ProviderProtocol::OpenaiCompletions,
    base_url: "https://api.deepseek.com",
    model: "deepseek-chat",
  },
];

//! `provider-anthropic` — 远程 Provider（Anthropic Messages API，SSE，`tool_use`/`tool_result`
//! content block 式工具调用，max_tokens 必填）。
//!
//! API key 走 SecretRef（`env:ANTHROPIC_API_KEY`），配置不落明文。

mod client;
mod stream;
mod wire;

pub use client::AnthropicProvider;

//! `provider-anthropic` — 远程 Provider（Anthropic Messages API，SSE，`tool_use`/`tool_result`
//! content block 式工具调用，max_tokens 必填）。
//!
//! 计划模块：`client` `stream`
//! 约束：API key 走 SecretRef（`env:ANTHROPIC_API_KEY`），配置不落明文。

// M6 待实施

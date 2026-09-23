//! `provider-openai` — 远程 Provider（OpenAI Chat Completions SSE，`tool_calls` 以 delta 流式
//! 到达、按 index 累积；忽略 reasoning 等未知字段）。
//!
//! API key 走 SecretRef（`env:OPENAI_API_KEY`），配置不落明文。

mod client;
mod stream;

pub use client::OpenAiProvider;

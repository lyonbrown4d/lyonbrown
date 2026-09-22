//! `provider-openai` — 远程 Provider（OpenAI Chat Completions SSE，`tool_calls` 以 delta 流式
//! 到达、需按 index 累积；忽略 reasoning 字段）。
//!
//! 计划模块：`client` `stream`
//! 约束：API key 走 SecretRef（`env:OPENAI_API_KEY`），配置不落明文。

// M6 待实施

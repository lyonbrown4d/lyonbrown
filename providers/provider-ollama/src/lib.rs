//! `provider-ollama` — 本地 Worker/Control Provider（Ollama HTTP API，`/api/chat` NDJSON 流）。
//!
//! 计划模块：`client` `stream`
//! 注意：qwen worker 需 `thinking: false`；结构化输出用 `format: "json"`；usage 从 `done` chunk 取。

// M2 待实施

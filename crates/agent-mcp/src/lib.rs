//! `agent-mcp` — rmcp client 桥接，MCP 工具并入 Tool Registry（§37，M7）。
//!
//! 计划模块：`bridge` `config`
//! 约束：rmcp 类型全部隔离在本 crate 内，不向其他 crate 泄漏；测试 fixture 为本地 Rust
//! stdio MCP server（`tests/` 内），不依赖 npx/网络。

// M7 待实施

//! `agent-storage` — SQLite 存储：session/event 日志、resume（§29，M8）。
//!
//! 计划模块：`db` `session_store`
//! 约束：rusqlite bundled；同步 API 经 spawn_blocking；WAL 模式。

// M8 待实施

//! `provider-llamacpp` — embedded llama.cpp Provider（M9 stretch，默认不启用）。
//!
//! 实施注意（M9）：
//! - 添加 workspace feature `llama-cpp`（默认关）与可选依赖 llama-cpp-rs（预编译 MSVC artifact）
//! - 本 crate 若需要 unsafe，在 Cargo.toml 覆盖 workspace lint：`unsafe_code = "allow"`
//! - 预编译 artifact 在本机有问题则砍掉 M9（Ollama 已覆盖本地层，不阻塞 MVP）

// M9 待实施（stretch）

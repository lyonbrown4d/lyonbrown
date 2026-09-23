# AGENTS.md

本文件约定 Lyonbrown 仓库的代码风格与结构约束，所有贡献者（人类与 AI agent）必须遵守。

## 约束

1. **单文件不超过 300 行**：任何单个源文件（含测试文件）不得超过 300 行。超出时按职责拆分为多个模块文件（`src/<name>/mod.rs` 或同目录多文件），而不是继续追加。
2. **缩进统一 2 空格**：通过 `rustfmt.toml` 的 `tab_spaces = 2` 统一，提交前运行 `cargo fmt --all`。禁止手动使用 Tab 或 4 空格缩进。
3. **实际代码与测试代码分离**：`src/` 内不得内嵌 `#[cfg(test)] mod tests`；测试一律放在对应 crate 的 `tests/` 目录（集成测试）或独立测试文件中。`src/` 只保留生产代码。
4. **约束统一收敛到本文件**：新增的代码风格 / 结构约束必须写入本文件，不散落在代码注释或口头约定中。

## 验证

- 提交前：`cargo fmt --all`、`cargo test --workspace`、`cargo clippy --workspace --all-targets` 全部通过。
- 行数检查：`find . -name '*.rs' -not -path './target/*' | xargs wc -l | awk '$1 > 300'` 应为空。

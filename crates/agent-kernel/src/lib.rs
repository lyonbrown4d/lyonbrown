//! `agent-kernel` — 事件驱动 Agent Kernel：状态机、agent loop、事件总线（§29-30）。
//!
//! 验收测试：`tests/e2e_mock.rs`（离线，规格见 `docs/zh/src/plans/mvp-implementation-plan.md` §7）

mod bus;
mod kernel;
mod session;
mod state;

pub use bus::EventBus;
pub use kernel::{Kernel, KernelBuilder, KernelLimits};
pub use session::Session;
pub use state::{KernelState, TransitionError};

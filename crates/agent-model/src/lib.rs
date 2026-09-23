//! `agent-model` — ModelProvider trait、ModelRegistry、ModelScheduler、QualityGate（§4/§9-11）。
//!
//! M1 提供 provider-neutral 接口、registry 和启发式 quality gate；调度策略在 M2 扩展。

mod gate;
mod registry;
mod scheduler;

pub mod provider;

pub use gate::{GateVerdict, HeuristicGate, QualityGate};
pub use provider::{
  ChatMessage, ModelError, ModelEvent, ModelProvider, ModelRequest, ModelStream, Role,
};
pub use registry::{ModelRegistry, RegistryError, SelectedModel};
pub use scheduler::{
  Complexity, ModelScheduler, RoutePhase, RoutingPolicy, SchedulerDecision, SchedulerError,
  SchedulerSelection, StepContext,
};

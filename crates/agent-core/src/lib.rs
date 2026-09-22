//! `agent-core` — 纯共享领域类型，无业务逻辑。
//!
//! 所有其它 crate 的依赖终点（严格 DAG 的根）。只放类型与错误，不放行为。

pub mod context;
pub mod error;
pub mod event;
pub mod ids;
pub mod model;
pub mod secret;
pub mod tool;

pub use context::{ContextItem, ContextItemKind, DataClassification};
pub use error::AgentError;
pub use event::{AgentEvent, StepOutcome};
pub use ids::{ObservationId, SessionId, StepId, ToolCallId, TurnId};
pub use model::{Locality, ModelCapabilities, ModelTier, StopReason, TokenUsage};
pub use secret::SecretRef;
pub use tool::{RiskLevel, ToolDefinition, ToolInvocation, ToolResult, ToolSource};

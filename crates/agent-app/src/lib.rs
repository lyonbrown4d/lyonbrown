//! UI-neutral application commands, runtime projections, and transport.

mod approval;
mod client;
mod command;
mod error;
mod projector;
mod runtime;
mod transport;
mod view;

pub use approval::{ApprovalInbox, RuntimeApprovalPrompt, approval_channel};
pub use client::{AgentClient, AgentClientError, AvailabilityKind, ProtocolErrorKind};
pub use command::{AgentCommand, ApprovalDecision};
pub use error::{RuntimeActorError, RuntimeCommandError, UnsupportedCommand};
pub use projector::Projector;
pub use runtime::{
  RuntimeShutdownHandle, RuntimeTask, spawn_runtime, spawn_runtime_waiting_for_model,
  spawn_runtime_with_approvals,
};
pub use transport::{AppController, RuntimeEndpoint, SnapshotSendError, channel};
pub use view::{
  ActivityPhase, ActivityView, ApprovalState, ApprovalView, ContextSummary, ManagedModelView,
  ModelFallbackView, ModelRoute, ModelSelection, ModelSetupAction, ModelSetupFailureView,
  ModelSetupPhase, ModelSetupSource, ModelSetupView, RouteSummary, RuntimePhase, RuntimeSnapshot,
  ToolActivityState, ToolActivityView, TurnStatus, TurnView,
};

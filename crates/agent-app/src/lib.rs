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
  spawn_runtime_with_approvals, spawn_runtime_with_approvals_and_models, spawn_runtime_with_models,
};
pub use transport::{AppController, RuntimeEndpoint, SnapshotSendError, channel};
pub use view::{
  ActivityPhase, ActivityView, ApprovalState, ApprovalView, ContextSummary, MAX_MODEL_CATALOG_SIZE,
  MAX_MODEL_ID_BYTES, MAX_MODEL_PROVIDER_BYTES, ManagedModelView, ModelCatalogError,
  ModelCatalogView, ModelFallbackView, ModelLocalityView, ModelOptionDisabledReason,
  ModelOptionView, ModelPolicyView, ModelRoute, ModelRuntimeView, ModelSelection, ModelSetupAction,
  ModelSetupFailureView, ModelSetupPhase, ModelSetupSource, ModelSetupView, ModelTierView,
  RouteSummary, RuntimePhase, RuntimeSnapshot, ToolActivityState, ToolActivityView, TurnStatus,
  TurnView,
};

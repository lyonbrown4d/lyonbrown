//! Embedded ComputerRuntime engine and authenticated interactive-client attachment.
//!
//! agentd owns run authority and durable intent/approval/receipt storage. Native
//! execution uses one blocking worker per attachment inside the interactive client;
//! there is no separate product process or Agent Loop. Reconnect never replays input.
//! Execute is trusted only after exact approval consumption and intent commit.
mod attachment;
mod client;
mod engine;
mod execution;
mod model;
mod native_lease;
pub mod wire;

pub use agent_computer::{DesktopTarget, TargetScope as DesktopScope};
#[cfg(feature = "native")]
pub use attachment::attach_native;
pub use attachment::{RuntimeAttachment, attach_host};
pub use client::{ComputerHostFactory, NativeComputerHost, computer_error_message};
pub use engine::{ComputerRuntime, MAX_TIMEOUT_MS};
pub use model::*;

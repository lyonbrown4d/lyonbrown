//! Durable host IO for Rig's serializable AgentRun state machine.
#![forbid(unsafe_code)]
mod budget;
mod classification;
mod context;
mod contracts;
mod control;
mod driver;
mod input;
mod model_io;
mod persistence;
mod privacy;
mod provider;
mod recovery;
mod runtime;
mod tools;

pub use classification::{CLASSIFICATION_OUTPUT_LIMIT, classify_json};
pub use context::{
  ContextStatus, SummaryInput, SummaryRequestGuard, cancel_unsent_summary, inspect_context,
  inspect_summary, undo_context,
};
pub use contracts::*;
pub use input::validate_input;
pub use recovery::{RecoveryReport, inspect_recovery};
pub use runtime::Runtime;

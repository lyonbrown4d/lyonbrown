//! Durable host IO for Rig's serializable AgentRun state machine.
#![forbid(unsafe_code)]
mod classification;
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
pub use contracts::*;
pub use recovery::{RecoveryReport, inspect_recovery};
pub use runtime::Runtime;

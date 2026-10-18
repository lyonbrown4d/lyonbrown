//! Reusable synthetic model HTTP fixtures. Depend on this crate only in tests.
mod fixtures;
mod server;
pub use fixtures::*;
pub use server::{MockModelServer, RecordedRequest, ScriptedReply};

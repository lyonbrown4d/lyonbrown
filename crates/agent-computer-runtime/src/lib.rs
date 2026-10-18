//! Embedded synchronous ComputerRuntime host engine, with no Agent Loop or app.
//!
//! The parent supplies authenticated private transport, one immutable native
//! desktop session, and durable intent/approval/receipt storage. Execute is a
//! trusted host command only after exact approval consumption and intent commit.
//! Run methods on a bounded blocking executor; reconnect must not replay input.
mod engine;
mod execution;
mod model;
pub mod wire;

pub use engine::{ComputerRuntime, MAX_TIMEOUT_MS};
pub use model::*;

//! The daemon's isolated adapter to the sole agent loop, `rpi-harness`.
#![forbid(unsafe_code)]

mod classification;
mod contracts;
mod event_projection;
mod events;
mod image_recovery;
mod input;
mod local_provider;
mod local_stream;
mod persistence;
mod provider;
mod runtime;
mod tools;

pub use classification::{CLASSIFICATION_OUTPUT_LIMIT, classify_json};
pub use contracts::*;
pub use runtime::Runtime;

//! Master-side routing. Classification can reduce authority, never grant it.
#![forbid(unsafe_code)]
mod backend;
mod contracts;
mod engine;
pub use backend::{LocalClassifier, RemoteClassifier};
pub use contracts::*;
pub use engine::DecisionEngine;

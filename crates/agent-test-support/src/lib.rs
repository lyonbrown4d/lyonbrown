//! Reusable synthetic model fixtures for tests and local development.
mod fixtures;
mod mock_inference;
mod server;
pub use fixtures::*;
pub use mock_inference::{
  MockGeneration, MockInferenceBackend, MockInferenceCounts, MockInferenceStep,
};
pub use server::{FiniteReply, MockModelServer, RecordedRequest, ScriptedReply};

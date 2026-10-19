//! Embedded advice only: the caller retains authorization, dispatch and durable state.
mod backend;
mod contract;
mod engine;
mod policy;
mod rules;
mod validation;
pub use backend::*;
pub use contract::*;
pub use engine::DecisionEngine;
pub use policy::*;

/// Reusable hard filter for adapters. The engine independently checks all returned advice.
pub fn eligible_model(model: &ModelCandidate, constraints: &Constraints) -> bool {
  validation::eligible(model, constraints)
}

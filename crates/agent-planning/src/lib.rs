//! Durable progress is advisory: a plan never grants execution capabilities.
#![forbid(unsafe_code)]
mod model;
mod transition;
mod validation;
pub use model::*;
pub use transition::{apply, record_verification};
pub use validation::validate_draft;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlanError {
  #[error("invalid plan: {0}")]
  Invalid(String),
  #[error("plan revision conflict")]
  Conflict,
  #[error("plan not found in this session and run")]
  NotFound,
  #[error("completion requires host verification or an explicit user override")]
  VerificationRequired,
  #[error("this plan operation requires a host or user authority")]
  Forbidden,
  #[error("request UUID was already used for different input")]
  RequestConflict,
}
pub const MAX_PLAN_BYTES: usize = 2 * 1024 * 1024;

pub type Result<T> = std::result::Result<T, PlanError>;

//! Bounded reviewable text changes. Filesystem effects belong to the host.
mod model;
mod preview;
pub use model::{MODEL_DIFF_BYTES, MODEL_VALIDATIONS, ModelChange, ModelChangeFile, model_change};
mod types;
pub use preview::{build_preview, digest, reverse_preview, validate};
pub use types::*;

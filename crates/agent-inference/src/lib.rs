//! Inference only: model execution never owns sessions, tools or an agent loop.
#![forbid(unsafe_code)]
mod contracts;
#[cfg(feature = "llama")]
mod llama;
#[cfg(feature = "mistral")]
mod mistral;
#[cfg(feature = "mistral")]
mod mistral_load;
mod registry;
pub use contracts::*;
#[cfg(feature = "llama")]
pub use llama::LlamaCppBackend;
#[cfg(feature = "mistral")]
pub use mistral::MistralBackend;
pub use registry::ModelRegistry;

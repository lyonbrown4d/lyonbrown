//! Inference only: model execution never owns sessions, tools or an agent loop.
#![forbid(unsafe_code)]
mod contracts;
#[cfg(feature = "llama")]
mod llama;
#[cfg(feature = "mistral")]
mod mistral;
#[cfg(feature = "mistral")]
mod mistral_load;
#[cfg(feature = "mistral")]
mod mistral_request;
#[cfg(all(test, feature = "mistral"))]
#[path = "../tests/support/mistral_request.rs"]
mod mistral_request_tests;
#[cfg(feature = "mistral")]
mod mistral_stream;
#[cfg(all(test, feature = "mistral"))]
#[path = "../tests/support/mistral_stream.rs"]
mod mistral_stream_tests;
mod registry;
pub use contracts::*;
#[cfg(feature = "llama")]
pub use llama::LlamaCppBackend;
#[cfg(feature = "mistral")]
pub use mistral::MistralBackend;
pub use registry::ModelRegistry;

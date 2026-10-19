//! Embedded semantic decisions with caller-owned embeddings and an immutable corpus.
//! No model, network client, or runtime is loaded by the default feature set.
mod backend;
mod embedding;
mod index;
#[cfg(feature = "semantic-local")]
mod local;

pub use backend::SemanticBackend;
pub use embedding::{EmbeddingDescriptor, EmbeddingProvider};
pub use index::{NegativeExample, SemanticConfig, SemanticExample};
#[cfg(feature = "semantic-local")]
pub use local::{LocalFastEmbed, OfflineEmbeddingConfig, OfflinePooling, VerifiedAsset};

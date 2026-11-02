//! Qdrant SDK boundaries for the memory vector-index port.
mod embedded;
mod embedded_init;
mod payload;
mod remote;
mod remote_filter;
mod remote_write;

pub use embedded::QdrantEmbeddedIndex;
pub use remote::QdrantRemoteIndex;

//! Ollama `/api/chat` adapter.
//!
//! Wire-specific details remain inside this crate; callers only see the provider-neutral
//! `agent-model` contract.

mod client;
mod stream;

pub use client::OllamaProvider;

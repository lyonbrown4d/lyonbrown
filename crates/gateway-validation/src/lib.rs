//! Shared, platform-independent validation for gateway form inputs.
//!
//! Borrowed inputs intentionally implement neither `Debug` nor serialization. Only fixed,
//! input-free field messages leave this crate; authorization and policy semantics stay server-side.
mod credentials;
mod policy;

pub use credentials::{CredentialErrors, Credentials};
pub use policy::{MAX_POLICY_DOCUMENT_BYTES, PolicyDraft, PolicyDraftErrors};

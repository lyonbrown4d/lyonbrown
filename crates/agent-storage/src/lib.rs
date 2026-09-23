//! SQLite event storage for session replay and audit.

mod db;
mod session_store;

pub use session_store::{EventWriter, SessionStore, SessionSummary, StoreError};

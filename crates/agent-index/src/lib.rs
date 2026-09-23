//! Incremental, workspace-scoped source index backed by SQLite FTS5.

mod index;
mod scan;
mod watcher;

pub use index::{IndexError, IndexedDocument, WorkspaceIndex};
pub use watcher::WorkspaceWatcher;

//! Full-screen diff reading surface.
//!
//! Modules: `model` holds the presentation types, `document` parses the daemon's unified diff,
//! `highlight` maps tree-sitter captures onto lines, `render` paints unified and side-by-side
//! modes, and `input` only mutates view state.

pub mod document;
pub mod highlight;
pub mod input;
pub mod model;
pub mod render;

pub use model::{DiffDocument, DiffMode, DiffStats, DiffViewState};

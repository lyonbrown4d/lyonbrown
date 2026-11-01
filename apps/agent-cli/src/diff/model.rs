//! Presentation model for the diff view.
//!
//! The daemon already returns `git diff` text, so clients parse and render it instead of
//! recomputing changes. These types carry no daemon authority and stay inside the terminal client.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffDocument {
  pub files: Vec<FileDiff>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
  pub old_path: String,
  pub new_path: String,
  pub hunks: Vec<Hunk>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hunk {
  pub header: String,
  pub old_start: u32,
  pub new_start: u32,
  pub lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineKind {
  #[default]
  Context,
  Added,
  Removed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffLine {
  pub kind: LineKind,
  pub old_line: Option<u32>,
  pub new_line: Option<u32>,
  pub text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffMode {
  #[default]
  Unified,
  SideBySide,
}

/// Scroll and mode state owned by the client view, never by the daemon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffViewState {
  pub mode: DiffMode,
  pub scroll: u16,
  pub h_scroll: u16,
  pub file: usize,
}

/// A loaded diff plus its view state. Read-only presentation data: the daemon never sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffView {
  pub path: String,
  pub document: DiffDocument,
  pub state: DiffViewState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffStats {
  pub files: usize,
  pub added: usize,
  pub removed: usize,
}

impl DiffDocument {
  pub fn stats(&self) -> DiffStats {
    DiffStats {
      files: self.files.len(),
      added: self
        .files
        .iter()
        .flat_map(|file| &file.hunks)
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| line.kind == LineKind::Added)
        .count(),
      removed: self
        .files
        .iter()
        .flat_map(|file| &file.hunks)
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| line.kind == LineKind::Removed)
        .count(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.files.iter().all(|file| file.hunks.is_empty())
  }
}

impl FileDiff {
  /// Pick the path to show: the new name unless the file was deleted.
  pub fn display_path(&self) -> &str {
    if self.new_path.is_empty() || self.new_path == "/dev/null" {
      &self.old_path
    } else {
      &self.new_path
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn stats_count_added_and_removed_lines_across_files() {
    let line = |kind| DiffLine {
      kind,
      ..DiffLine::default()
    };
    let document = DiffDocument {
      files: vec![
        FileDiff {
          new_path: "a.rs".into(),
          hunks: vec![Hunk {
            lines: vec![
              line(LineKind::Context),
              line(LineKind::Added),
              line(LineKind::Removed),
            ],
            ..Hunk::default()
          }],
          ..FileDiff::default()
        },
        FileDiff {
          new_path: "b.rs".into(),
          hunks: vec![Hunk {
            lines: vec![line(LineKind::Added)],
            ..Hunk::default()
          }],
          ..FileDiff::default()
        },
      ],
    };
    assert_eq!(
      document.stats(),
      DiffStats {
        files: 2,
        added: 2,
        removed: 1
      }
    );
    assert!(!document.is_empty());
  }

  #[test]
  fn deleted_files_keep_their_old_path() {
    let file = FileDiff {
      old_path: "gone.rs".into(),
      new_path: "/dev/null".into(),
      ..FileDiff::default()
    };
    assert_eq!(file.display_path(), "gone.rs");
  }
}

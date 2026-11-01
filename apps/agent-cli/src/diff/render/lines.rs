//! Logical rows for the diff view.
//!
//! One row is one painted screen line in either mode. Rows borrow their text from the document
//! and refer to syntax spans by arena index, so a large diff never copies line text.

use crate::diff::{
  highlight::{self, HighlightSpan, Syntax},
  model::{DiffDocument, DiffLine, DiffMode, FileDiff, Hunk, LineKind},
};

/// One side of a side-by-side row.
#[derive(Clone, Copy)]
pub(super) struct SideCell<'a> {
  pub number: Option<u32>,
  pub text: &'a str,
  pub spans: Option<usize>,
  pub kind: LineKind,
}

/// A unified row keeps both line numbers plus the marker that introduces its text.
#[derive(Clone, Copy)]
pub(super) struct UnifiedRow<'a> {
  pub old: Option<u32>,
  pub new: Option<u32>,
  pub marker: char,
  pub text: &'a str,
  pub spans: Option<usize>,
  pub kind: LineKind,
}

/// A side-by-side row; a missing side is padded with blank space.
#[derive(Clone, Copy)]
pub(super) struct SplitRow<'a> {
  pub left: Option<SideCell<'a>>,
  pub right: Option<SideCell<'a>>,
}

/// One painted line of the view.
pub(super) enum Row<'a> {
  File { path: &'a str },
  Hunk { header: &'a str },
  Unified(UnifiedRow<'a>),
  Split(SplitRow<'a>),
}

/// Rows for a whole document plus the syntax spans those rows point at.
#[derive(Default)]
pub(super) struct Layout<'a> {
  pub rows: Vec<Row<'a>>,
  spans: Vec<Vec<HighlightSpan>>,
}

impl<'a> Layout<'a> {
  /// Builds every row up front so painting only slices text that already exists.
  pub(super) fn build(document: &'a DiffDocument, mode: DiffMode) -> Self {
    let mut layout = Self::default();
    for file in &document.files {
      layout.rows.push(Row::File {
        path: file.display_path(),
      });
      let mut cursor = layout.highlight(file);
      for hunk in &file.hunks {
        layout.rows.push(Row::Hunk {
          header: &hunk.header,
        });
        match mode {
          DiffMode::Unified => push_unified(&mut layout.rows, hunk, &mut cursor),
          DiffMode::SideBySide => push_split(&mut layout.rows, hunk, &mut cursor),
        }
      }
    }
    layout
  }

  /// Highlight spans of one row; an absent or stale index simply means no colouring.
  pub(super) fn span(&self, id: Option<usize>) -> &[HighlightSpan] {
    id.and_then(|index| self.spans.get(index))
      .map_or(&[], Vec::as_slice)
  }

  /// Highlights both sides of one file and returns the cursors into the span arena.
  fn highlight(&mut self, file: &'a FileDiff) -> Cursor {
    let syntax = Syntax::from_path(file.display_path());
    let new_text = side_text(file, Side::New);
    let old_text = side_text(file, Side::Old);
    let new_spans = highlight::highlight_lines(syntax, &new_text);
    let old_spans = highlight::highlight_lines(syntax, &old_text);
    let mut cursor = Cursor {
      new_base: self.spans.len(),
      new_count: new_spans.len(),
      ..Cursor::default()
    };
    self.spans.extend(new_spans);
    cursor.old_base = self.spans.len();
    cursor.old_count = old_spans.len();
    self.spans.extend(old_spans);
    cursor
  }
}

/// Rows one file contributes: its header, then every hunk header and its lines.
pub(super) fn file_row_count(file: &FileDiff, mode: DiffMode) -> usize {
  1 + file
    .hunks
    .iter()
    .map(|hunk| 1 + hunk_row_count(hunk, mode))
    .sum::<usize>()
}

/// Rows one hunk contributes, excluding its own header.
pub(super) fn hunk_row_count(hunk: &Hunk, mode: DiffMode) -> usize {
  match mode {
    DiffMode::Unified => hunk.lines.len(),
    DiffMode::SideBySide => {
      let mut count = 0;
      let mut index = 0;
      while index < hunk.lines.len() {
        if hunk.lines[index].kind == LineKind::Context {
          count += 1;
          index += 1;
          continue;
        }
        let removed = run_len(&hunk.lines, index, LineKind::Removed);
        let added = run_len(&hunk.lines, index + removed, LineKind::Added);
        if removed + added == 0 {
          index += 1;
          continue;
        }
        count += removed.max(added);
        index += removed + added;
      }
      count
    }
  }
}

/// Cursors into the per-file span arenas; both advance in document order.
#[derive(Default)]
struct Cursor {
  new_base: usize,
  new_count: usize,
  old_base: usize,
  old_count: usize,
  new: usize,
  old: usize,
}

impl Cursor {
  fn new_span(&self, offset: usize) -> Option<usize> {
    let index = self.new + offset;
    (index < self.new_count).then_some(self.new_base + index)
  }

  fn old_span(&self, offset: usize) -> Option<usize> {
    let index = self.old + offset;
    (index < self.old_count).then_some(self.old_base + index)
  }
}

fn push_unified<'a>(rows: &mut Vec<Row<'a>>, hunk: &'a Hunk, cursor: &mut Cursor) {
  for line in &hunk.lines {
    let spans = match line.kind {
      LineKind::Removed => {
        let span = cursor.old_span(0);
        cursor.old += 1;
        span
      }
      LineKind::Added => {
        let span = cursor.new_span(0);
        cursor.new += 1;
        span
      }
      LineKind::Context => {
        let span = cursor.new_span(0);
        cursor.new += 1;
        cursor.old += 1;
        span
      }
    };
    rows.push(Row::Unified(UnifiedRow {
      old: line.old_line,
      new: line.new_line,
      marker: marker(line.kind),
      text: &line.text,
      spans,
      kind: line.kind,
    }));
  }
}

/// Pairs the removed run of a hunk with the added run that follows it, one row per pair.
fn push_split<'a>(rows: &mut Vec<Row<'a>>, hunk: &'a Hunk, cursor: &mut Cursor) {
  let lines = hunk.lines.as_slice();
  let mut index = 0;
  while index < lines.len() {
    if lines[index].kind == LineKind::Context {
      let line = &lines[index];
      let span = cursor.new_span(0);
      cursor.new += 1;
      cursor.old += 1;
      rows.push(Row::Split(SplitRow {
        left: Some(cell(line, line.old_line, span)),
        right: Some(cell(line, line.new_line, span)),
      }));
      index += 1;
      continue;
    }
    let removed = run_len(lines, index, LineKind::Removed);
    let added = run_len(lines, index + removed, LineKind::Added);
    if removed + added == 0 {
      index += 1;
      continue;
    }
    let removed_start = index;
    let added_start = index + removed;
    for offset in 0..removed.max(added) {
      let left = (offset < removed).then(|| {
        let line = &lines[removed_start + offset];
        SideCell {
          number: line.old_line,
          text: &line.text,
          spans: cursor.old_span(offset),
          kind: LineKind::Removed,
        }
      });
      let right = (offset < added).then(|| {
        let line = &lines[added_start + offset];
        SideCell {
          number: line.new_line,
          text: &line.text,
          spans: cursor.new_span(offset),
          kind: LineKind::Added,
        }
      });
      rows.push(Row::Split(SplitRow { left, right }));
    }
    cursor.old += removed;
    cursor.new += added;
    index = added_start + added;
  }
}

fn cell<'a>(line: &'a DiffLine, number: Option<u32>, spans: Option<usize>) -> SideCell<'a> {
  SideCell {
    number,
    text: &line.text,
    spans,
    kind: line.kind,
  }
}

fn marker(kind: LineKind) -> char {
  match kind {
    LineKind::Context => ' ',
    LineKind::Added => '+',
    LineKind::Removed => '-',
  }
}

/// Number of leading lines of `kind` in `lines[start..]`.
fn run_len(lines: &[DiffLine], start: usize, kind: LineKind) -> usize {
  lines.get(start..).map_or(0, |rest| {
    rest.iter().take_while(|line| line.kind == kind).count()
  })
}

/// Which side of the document a line belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
  Old,
  New,
}

impl Side {
  fn keeps(self, kind: LineKind) -> bool {
    !matches!(
      (self, kind),
      (Side::Old, LineKind::Added) | (Side::New, LineKind::Removed)
    )
  }
}

fn side_text(file: &FileDiff, side: Side) -> Vec<&str> {
  file
    .hunks
    .iter()
    .flat_map(|hunk| &hunk.lines)
    .filter(|line| side.keeps(line.kind))
    .map(|line| line.text.as_str())
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::diff::model::{DiffDocument, FileDiff, Hunk};

  fn line(kind: LineKind, text: &str) -> DiffLine {
    DiffLine {
      kind,
      text: text.into(),
      ..DiffLine::default()
    }
  }

  fn document(lines: Vec<DiffLine>) -> DiffDocument {
    DiffDocument {
      files: vec![FileDiff {
        old_path: "src/a.rs".into(),
        new_path: "src/a.rs".into(),
        hunks: vec![Hunk {
          header: "@@ -1,3 +1,3 @@".into(),
          lines,
          ..Hunk::default()
        }],
      }],
    }
  }

  /// Text of both cells in every side-by-side row, file and hunk headers excluded.
  fn split_rows(document: &DiffDocument) -> Vec<(Option<&str>, Option<&str>)> {
    let layout = Layout::build(document, DiffMode::SideBySide);
    layout
      .rows
      .iter()
      .filter_map(|row| match row {
        Row::Split(split) => Some((
          split.left.map(|cell| cell.text),
          split.right.map(|cell| cell.text),
        )),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn split_pairs_every_removed_with_the_next_added() {
    use LineKind::{Added, Context, Removed};
    let document = document(vec![
      line(Context, "fn main() {"),
      line(Removed, "old();"),
      line(Added, "new();"),
      line(Context, "}"),
    ]);
    assert_eq!(
      split_rows(&document),
      vec![
        (Some("fn main() {"), Some("fn main() {")),
        (Some("old();"), Some("new();")),
        (Some("}"), Some("}")),
      ]
    );
  }

  #[test]
  fn split_pads_the_short_side_when_removed_outnumber_added() {
    use LineKind::{Added, Removed};
    let document = document(vec![
      line(Removed, "one();"),
      line(Removed, "two();"),
      line(Added, "three();"),
    ]);
    assert_eq!(
      split_rows(&document),
      vec![(Some("one();"), Some("three();")), (Some("two();"), None)]
    );
  }

  #[test]
  fn split_pads_the_short_side_when_added_outnumber_removed() {
    use LineKind::{Added, Removed};
    let document = document(vec![
      line(Removed, "one();"),
      line(Added, "two();"),
      line(Added, "three();"),
    ]);
    assert_eq!(
      split_rows(&document),
      vec![(Some("one();"), Some("two();")), (None, Some("three();"))]
    );
  }

  #[test]
  fn split_keeps_context_only_hunks_on_one_row_each() {
    use LineKind::Context;
    let document = document(vec![line(Context, "a"), line(Context, "b")]);
    assert_eq!(
      split_rows(&document),
      vec![(Some("a"), Some("a")), (Some("b"), Some("b"))]
    );
  }

  #[test]
  fn row_counts_match_built_rows_in_both_modes() {
    use LineKind::{Added, Context, Removed};
    let document = document(vec![
      line(Context, "a"),
      line(Removed, "b"),
      line(Removed, "c"),
      line(Added, "d"),
      line(Context, "e"),
    ]);
    for mode in [DiffMode::Unified, DiffMode::SideBySide] {
      let layout = Layout::build(&document, mode);
      let counted = document
        .files
        .iter()
        .map(|file| file_row_count(file, mode))
        .sum::<usize>();
      assert_eq!(layout.rows.len(), counted);
    }
  }

  #[test]
  fn unified_rows_keep_numbers_markers_and_spans() {
    use LineKind::{Added, Context, Removed};
    let mut lines = vec![
      line(Context, "keep"),
      line(Removed, "gone"),
      line(Added, "born"),
    ];
    lines[0].old_line = Some(1);
    lines[0].new_line = Some(1);
    lines[1].old_line = Some(2);
    lines[2].new_line = Some(2);
    let document = document(lines);
    let layout = Layout::build(&document, DiffMode::Unified);
    let rows: Vec<_> = layout
      .rows
      .iter()
      .filter_map(|row| match row {
        Row::Unified(line) => Some((line.marker, line.old, line.new)),
        _ => None,
      })
      .collect();
    assert_eq!(
      rows,
      vec![
        (' ', Some(1), Some(1)),
        ('-', Some(2), None),
        ('+', None, Some(2))
      ]
    );
  }
}

//! Diff rendering in unified and side-by-side modes.
//!
//! The caller owns the frame, the title bar and the key hint line; this module only fills `area`.
//! Rows are built per frame because the view state, not a widget cache, drives scrolling.

mod lines;
mod paint;

use ratatui::{
  Frame,
  buffer::Buffer,
  layout::Rect,
  style::{Modifier, Style},
};

use crate::{
  diff::model::{DiffDocument, DiffMode, DiffViewState, LineKind},
  theme::Theme,
};

use lines::{Layout, Row, SideCell, SplitRow, UnifiedRow};

/// Below this width a split view cannot show code, so it falls back to unified.
const MIN_SPLIT_WIDTH: u16 = 40;
/// Line numbers occupy four columns.
const NUMBER_WIDTH: usize = 4;
/// Width of `"1234 1234 │ + "`.
const UNIFIED_PREFIX: u16 = 2 * NUMBER_WIDTH as u16 + 6;
/// Width of `"1234 "`.
const SIDE_PREFIX: u16 = NUMBER_WIDTH as u16 + 1;

/// Logical row count for scrolling bounds; independent of the terminal height.
pub fn total_rows(document: &DiffDocument, mode: DiffMode) -> usize {
  document
    .files
    .iter()
    .map(|file| lines::file_row_count(file, mode))
    .sum()
}

/// Paints the diff body; the caller owns the surrounding frame and status line.
pub fn render(
  frame: &mut Frame,
  area: Rect,
  document: &DiffDocument,
  state: &DiffViewState,
  theme: Theme,
) {
  if area.is_empty() {
    return;
  }
  let layout = Layout::build(document, resolve_mode(state.mode, area.width));
  let visible = usize::from(area.height);
  let scroll = usize::from(state.scroll).min(layout.rows.len().saturating_sub(visible));
  let skip = usize::from(state.h_scroll);
  let buffer = frame.buffer_mut();
  for (offset, row) in layout.rows.iter().skip(scroll).take(visible).enumerate() {
    let Some(offset) = u16::try_from(offset).ok() else {
      break;
    };
    let y = area.y.saturating_add(offset);
    if y >= area.bottom() {
      break;
    }
    paint_row(
      buffer,
      Rect::new(area.x, y, area.width, 1),
      &layout,
      row,
      &theme,
      skip,
    );
  }
}

/// Side-by-side needs room for two code columns; narrower areas stay unified.
fn resolve_mode(mode: DiffMode, width: u16) -> DiffMode {
  match mode {
    DiffMode::SideBySide if width < MIN_SPLIT_WIDTH => DiffMode::Unified,
    other => other,
  }
}

fn paint_row(
  buffer: &mut Buffer,
  area: Rect,
  layout: &Layout<'_>,
  row: &Row<'_>,
  theme: &Theme,
  skip: usize,
) {
  match row {
    Row::File { path } => paint::paint(
      buffer,
      area,
      &format!("▸ {path}"),
      &[],
      Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
      theme,
      skip,
    ),
    Row::Hunk { header } => paint::paint(
      buffer,
      area,
      header,
      &[],
      Style::new().fg(theme.diff.hunk),
      theme,
      skip,
    ),
    Row::Unified(line) => paint_unified(buffer, area, layout, line, theme, skip),
    Row::Split(split) => paint_split(buffer, area, layout, split, theme, skip),
  }
}

fn paint_unified(
  buffer: &mut Buffer,
  area: Rect,
  layout: &Layout<'_>,
  row: &UnifiedRow<'_>,
  theme: &Theme,
  skip: usize,
) {
  let base = line_style(row.kind, theme);
  let prefix = format!(
    "{:>width$} {:>width$} │ {} ",
    number(row.old),
    number(row.new),
    row.marker,
    width = NUMBER_WIDTH
  );
  buffer.set_stringn(
    area.x,
    area.y,
    prefix,
    usize::from(area.width),
    gutter_style(base, theme),
  );
  let content = Rect::new(
    area.x.saturating_add(UNIFIED_PREFIX),
    area.y,
    area.width.saturating_sub(UNIFIED_PREFIX),
    1,
  );
  if content.is_empty() {
    return;
  }
  paint::paint(
    buffer,
    content,
    row.text,
    layout.span(row.spans),
    base,
    theme,
    skip,
  );
}

fn paint_split(
  buffer: &mut Buffer,
  area: Rect,
  layout: &Layout<'_>,
  row: &SplitRow<'_>,
  theme: &Theme,
  skip: usize,
) {
  let left_width = area.width.saturating_sub(1) / 2;
  let right_width = area.width.saturating_sub(1).saturating_sub(left_width);
  if area.width > 1 {
    buffer.set_stringn(
      area.x.saturating_add(left_width),
      area.y,
      "│",
      1,
      Style::new().fg(theme.muted),
    );
  }
  paint_cell(
    buffer,
    Rect::new(area.x, area.y, left_width, 1),
    layout,
    row.left,
    theme,
    skip,
  );
  paint_cell(
    buffer,
    Rect::new(
      area.x.saturating_add(left_width).saturating_add(1),
      area.y,
      right_width,
      1,
    ),
    layout,
    row.right,
    theme,
    skip,
  );
}

fn paint_cell(
  buffer: &mut Buffer,
  area: Rect,
  layout: &Layout<'_>,
  cell: Option<SideCell<'_>>,
  theme: &Theme,
  skip: usize,
) {
  let Some(cell) = cell else {
    return;
  };
  if area.is_empty() {
    return;
  }
  let base = line_style(cell.kind, theme);
  buffer.set_stringn(
    area.x,
    area.y,
    format!("{:>width$} ", number(cell.number), width = NUMBER_WIDTH),
    usize::from(area.width),
    gutter_style(base, theme),
  );
  let content = Rect::new(
    area.x.saturating_add(SIDE_PREFIX),
    area.y,
    area.width.saturating_sub(SIDE_PREFIX),
    1,
  );
  if content.is_empty() {
    return;
  }
  paint::paint(
    buffer,
    content,
    cell.text,
    layout.span(cell.spans),
    base,
    theme,
    skip,
  );
}

/// Added and removed lines carry a tinted background for the whole row.
fn line_style(kind: LineKind, theme: &Theme) -> Style {
  match kind {
    LineKind::Added => Style::new().fg(theme.diff.added).bg(theme.diff.added_bg),
    LineKind::Removed => Style::new()
      .fg(theme.diff.removed)
      .bg(theme.diff.removed_bg),
    LineKind::Context => Style::new().fg(theme.text),
  }
}

/// Gutters stay quiet but keep the row tint, so a tinted line reads as one block.
fn gutter_style(base: Style, theme: &Theme) -> Style {
  let style = Style::new().fg(theme.muted);
  match base.bg {
    Some(background) => style.bg(background),
    None => style,
  }
}

fn number(value: Option<u32>) -> String {
  value.map(|value| value.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    config::ThemeChoice,
    diff::model::{DiffLine, FileDiff, Hunk},
  };
  use ratatui::{Terminal, backend::TestBackend};

  fn theme() -> Theme {
    Theme::resolve(ThemeChoice::Dark, None)
  }

  fn line(kind: LineKind, old: Option<u32>, new: Option<u32>, text: &str) -> DiffLine {
    DiffLine {
      kind,
      old_line: old,
      new_line: new,
      text: text.into(),
    }
  }

  fn document() -> DiffDocument {
    DiffDocument {
      files: vec![FileDiff {
        old_path: "src/a.rs".into(),
        new_path: "src/a.rs".into(),
        hunks: vec![Hunk {
          header: "@@ -1,3 +1,4 @@".into(),
          old_start: 1,
          new_start: 1,
          lines: vec![
            line(LineKind::Context, Some(1), Some(1), "fn demo() {"),
            line(LineKind::Removed, Some(2), None, "  old();"),
            line(LineKind::Added, None, Some(2), "  new();"),
            line(LineKind::Added, None, Some(3), "  extra();"),
            line(LineKind::Context, Some(3), Some(4), "}"),
          ],
        }],
      }],
    }
  }

  fn screen(document: &DiffDocument, state: &DiffViewState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
      .draw(|frame| render(frame, frame.area(), document, state, theme()))
      .expect("draw");
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
      for x in 0..buffer.area.width {
        text.push_str(buffer[(x, y)].symbol());
      }
      text.push('\n');
    }
    text
  }

  fn find<'a>(screen: &'a str, needle: &str) -> &'a str {
    screen
      .lines()
      .find(|line| line.contains(needle))
      .expect("line on screen")
  }

  #[test]
  fn total_rows_counts_headers_hunks_and_lines() {
    let document = document();
    assert!(total_rows(&document, DiffMode::Unified) > 0);
    assert_eq!(total_rows(&document, DiffMode::Unified), 7);
    assert_eq!(total_rows(&document, DiffMode::SideBySide), 6);
    let built = Layout::build(&document, DiffMode::SideBySide);
    assert_eq!(built.rows.len(), 6);
  }

  #[test]
  fn unified_paints_path_hunk_header_and_markers() {
    let document = document();
    let state = DiffViewState::default();
    let screen = screen(&document, &state, 80, 24);
    assert!(total_rows(&document, DiffMode::Unified) > 0);
    assert!(screen.contains("▸ src/a.rs"));
    assert!(screen.contains("@@ -1,3 +1,4 @@"));
    assert!(find(&screen, "old();").contains("│ -"));
    assert!(find(&screen, "new();").contains("│ +"));
  }

  #[test]
  fn side_by_side_paints_path_and_pairs_removed_with_added() {
    let document = document();
    let state = DiffViewState {
      mode: DiffMode::SideBySide,
      ..DiffViewState::default()
    };
    let screen = screen(&document, &state, 80, 24);
    assert!(screen.contains("▸ src/a.rs"));
    let row = find(&screen, "old();");
    assert!(row.contains("new();"));
    assert!(find(&screen, "fn demo() {").contains("│"));
  }

  #[test]
  fn narrow_areas_fall_back_to_unified() {
    let document = document();
    let state = DiffViewState {
      mode: DiffMode::SideBySide,
      ..DiffViewState::default()
    };
    let screen = screen(&document, &state, 30, 24);
    assert!(screen.contains("▸ src/a.rs"));
    assert!(find(&screen, "old();").contains("│ -"));
  }

  #[test]
  fn horizontal_scroll_keeps_text_inside_the_row() {
    let document = document();
    let state = DiffViewState {
      h_scroll: 6,
      ..DiffViewState::default()
    };
    let screen = screen(&document, &state, 80, 24);
    let scrolled = find(&screen, "│ -");
    assert!(!scrolled.contains("old();"));
    assert!(scrolled.contains(");"));
  }

  #[test]
  fn scrolling_past_the_end_is_clamped() {
    let document = document();
    let state = DiffViewState {
      scroll: u16::MAX,
      ..DiffViewState::default()
    };
    let screen = screen(&document, &state, 80, 3);
    assert!(!screen.contains("▸ src/a.rs"));
    assert!(find(&screen, "extra();").contains("│ +"));
  }

  #[test]
  fn tiny_areas_and_empty_documents_render_without_panicking() {
    let document = document();
    let state = DiffViewState {
      mode: DiffMode::SideBySide,
      ..DiffViewState::default()
    };
    for (width, height) in [(20, 1), (40, 2), (41, 3)] {
      let painted = screen(&document, &state, width, height);
      assert!(!painted.trim().is_empty());
    }
    let empty = DiffDocument::default();
    assert_eq!(total_rows(&empty, DiffMode::Unified), 0);
    assert!(!screen(&empty, &DiffViewState::default(), 80, 24).contains("▸"));
  }
}

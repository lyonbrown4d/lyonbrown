//! Text painting for diff rows: syntax spans, display-column cropping and tinted padding.

use ratatui::{
  buffer::Buffer,
  layout::Rect,
  style::{Color, Style},
  text::{Line, Span},
};

use crate::{
  diff::highlight::{Capture, HighlightSpan},
  theme::{SyntaxColors, Theme},
};

/// Paints `text` into `area`, cropping `skip` display columns from its left edge.
pub(super) fn paint(
  buffer: &mut Buffer,
  area: Rect,
  text: &str,
  spans: &[HighlightSpan],
  base: Style,
  theme: &Theme,
  skip: usize,
) {
  let width = usize::from(area.width);
  let mut crop = Crop::new(skip, width);
  let mut line = Vec::new();
  for (segment, style) in segments(text, spans, base, theme) {
    if crop.done {
      break;
    }
    crop.push(&mut line, segment, style);
  }
  if crop.used < width {
    line.push(Span::styled(" ".repeat(width - crop.used), base));
  }
  buffer.set_line(area.x, area.y, &Line::from(line), area.width);
}

/// Splits a line into styled runs; syntax spans win, gaps keep the diff tint.
fn segments<'a>(
  text: &'a str,
  spans: &[HighlightSpan],
  base: Style,
  theme: &Theme,
) -> Vec<(&'a str, Style)> {
  let mut result = Vec::new();
  let mut cursor = 0;
  for span in spans {
    let start = span.start.min(text.len());
    let end = span.end.min(text.len());
    let usable =
      start >= cursor && end > start && text.is_char_boundary(start) && text.is_char_boundary(end);
    if !usable {
      continue;
    }
    if start > cursor {
      result.push((&text[cursor..start], base));
    }
    result.push((
      &text[start..end],
      Style::new().fg(capture_color(&theme.syntax, span.capture)),
    ));
    cursor = end;
  }
  if cursor < text.len() {
    result.push((&text[cursor..], base));
  }
  result
}

/// Colour for one tree-sitter capture.
fn capture_color(colors: &SyntaxColors, capture: Capture) -> Color {
  match capture {
    Capture::Attribute => colors.attribute,
    Capture::Comment => colors.comment,
    Capture::Constant => colors.constant,
    Capture::Function => colors.function,
    Capture::Keyword => colors.keyword,
    Capture::Number => colors.number,
    Capture::Operator => colors.operator,
    Capture::Property => colors.property,
    Capture::Punctuation => colors.punctuation,
    Capture::String => colors.string,
    Capture::Type => colors.type_name,
    Capture::Variable => colors.variable,
  }
}

/// Crops a row to the visible display columns, honouring a horizontal offset.
struct Crop {
  skip: usize,
  used: usize,
  limit: usize,
  done: bool,
}

impl Crop {
  fn new(skip: usize, limit: usize) -> Self {
    Self {
      skip,
      used: 0,
      limit,
      done: limit == 0,
    }
  }

  /// Appends the visible part of one styled segment, splitting wide glyphs never.
  fn push<'a>(&mut self, out: &mut Vec<Span<'a>>, segment: &'a str, style: Style) {
    let mut start = 0;
    for (index, glyph) in segment.char_indices() {
      let width = char_width(glyph);
      if self.skip > 0 {
        if width <= self.skip {
          self.skip -= width;
          start = index + glyph.len_utf8();
          continue;
        }
        // A wide glyph straddles the offset; keeping it whole beats dropping it.
        self.skip = 0;
      }
      if self.used + width > self.limit {
        if start < index {
          out.push(Span::styled(&segment[start..index], style));
        }
        self.done = true;
        return;
      }
      self.used += width;
    }
    if start < segment.len() {
      out.push(Span::styled(&segment[start..], style));
    }
  }
}

/// Display width of one glyph, measured the way Ratatui measures a span.
fn char_width(glyph: char) -> usize {
  let mut buffer = [0u8; 4];
  let text: &str = glyph.encode_utf8(&mut buffer);
  Span::raw(text).width()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::ThemeChoice;

  fn theme() -> Theme {
    Theme::resolve(ThemeChoice::Dark, None)
  }

  fn cropped(text: &str, skip: usize, limit: usize) -> String {
    let mut crop = Crop::new(skip, limit);
    let mut line = Vec::new();
    crop.push(&mut line, text, Style::new());
    line.iter().map(|span| span.content.as_ref()).collect()
  }

  #[test]
  fn cropping_counts_display_columns_for_cjk_text() {
    assert_eq!(cropped("中文abc", 0, 4), "中文");
    assert_eq!(cropped("中文abc", 4, 8), "abc");
    assert_eq!(cropped("abcdef", 2, 3), "cde");
  }

  #[test]
  fn spans_outside_the_line_are_ignored() {
    let spans = [
      HighlightSpan {
        start: 20,
        end: 30,
        capture: Capture::Keyword,
      },
      HighlightSpan {
        start: 2,
        end: 4,
        capture: Capture::String,
      },
    ];
    let theme = theme();
    let segments = segments("let x = 1;", &spans, Style::new(), &theme);
    let text: String = segments.iter().map(|(text, _)| *text).collect();
    assert_eq!(text, "let x = 1;");
  }
}

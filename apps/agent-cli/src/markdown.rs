//! Theme adaptation through tui-markdown's public style-sheet interface.
use ratatui::{
  style::{Modifier, Style},
  text::{Line, Span, Text},
};
use tui_markdown::{AlertKind, Options, StyleSheet};

use crate::theme::Theme;

pub(crate) fn render(content: &str, theme: Theme) -> Text<'static> {
  let options = Options::new(TerminalStyles(theme));
  let parsed = tui_markdown::from_str_with_options(content, &options);
  Text::from(
    parsed
      .lines
      .into_iter()
      .map(|line| {
        let mut owned = Line::from(
          line
            .spans
            .into_iter()
            .map(|span| Span::styled(span.content.into_owned(), span.style))
            .collect::<Vec<_>>(),
        );
        owned.style = line.style;
        owned
      })
      .collect::<Vec<_>>(),
  )
}

#[derive(Clone, Copy)]
struct TerminalStyles(Theme);

impl TerminalStyles {
  fn body(self) -> Style {
    Style::default().fg(self.0.text).bg(self.0.background)
  }
  fn accent(self) -> Style {
    self.body().fg(self.0.accent)
  }
  fn muted(self) -> Style {
    self.body().fg(self.0.muted)
  }
}

impl StyleSheet for TerminalStyles {
  fn heading(&self, _level: u8) -> Style {
    self.accent().add_modifier(Modifier::BOLD)
  }
  fn code(&self) -> Style {
    self.accent()
  }
  fn link(&self) -> Style {
    self.accent().add_modifier(Modifier::UNDERLINED)
  }
  fn blockquote(&self) -> Style {
    self.muted().add_modifier(Modifier::ITALIC)
  }
  fn heading_meta(&self) -> Style {
    self.muted()
  }
  fn metadata_block(&self) -> Style {
    self.muted()
  }
  fn heading_marker(&self, _level: u8) -> &str {
    ""
  }
  fn code_block_fence(&self) -> &str {
    ""
  }
  fn html(&self) -> Style {
    self.muted()
  }
  fn math_inline(&self) -> Style {
    self.accent().add_modifier(Modifier::ITALIC)
  }
  fn math_display(&self) -> Style {
    self.accent()
  }
  fn footnote_ref(&self) -> Style {
    self.muted()
  }
  fn footnote_def(&self) -> Style {
    self.muted()
  }
  fn definition_term(&self) -> Style {
    self.body().add_modifier(Modifier::BOLD)
  }
  fn definition_description(&self) -> Style {
    self.body()
  }
  fn alert(&self, kind: AlertKind) -> Style {
    match kind {
      AlertKind::Warning | AlertKind::Caution => self.body().fg(self.0.warning),
      _ => self.accent(),
    }
  }
  fn alert_icon(&self, _kind: AlertKind) -> &str {
    ""
  }
  fn table_header(&self) -> Style {
    self.accent().add_modifier(Modifier::BOLD)
  }
  fn table_cell(&self) -> Style {
    self.body()
  }
  fn table_border(&self) -> Style {
    self.muted()
  }
  fn list_marker(&self) -> Style {
    self.accent()
  }
  fn image_alt(&self) -> Style {
    self.muted().add_modifier(Modifier::ITALIC)
  }
}

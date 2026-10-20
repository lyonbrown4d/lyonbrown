use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect, style::Style};
use ratatui_textarea::{TextArea, WrapMode};

/// Client-owned editor state. Protocol text is materialized only at request boundaries.
#[derive(Debug)]
pub struct Composer(TextArea<'static>);

impl Default for Composer {
  fn default() -> Self {
    let mut editor = TextArea::default();
    editor.set_cursor_line_style(Style::default());
    editor.set_wrap_mode(WrapMode::WordOrGlyph);
    Self(editor)
  }
}

impl From<&str> for Composer {
  fn from(text: &str) -> Self {
    let mut composer = Self::default();
    composer.paste(text);
    composer
  }
}

impl Composer {
  pub fn text(&self) -> String {
    self.0.lines().join("\n")
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  pub fn clear(&mut self) {
    // Acknowledged prompts are no longer drafts, including in undo history.
    *self = Self::default();
  }

  pub fn cursor(&self) -> (usize, usize) {
    let cursor = self.0.cursor();
    (cursor.0, cursor.1)
  }

  pub(crate) fn key(&mut self, key: KeyEvent) {
    self.0.input(key);
  }

  pub(crate) fn newline(&mut self) {
    self.0.insert_newline();
  }

  pub(crate) fn paste(&mut self, text: &str) {
    self.0.insert_str(text);
  }

  pub(crate) fn render(&self, frame: &mut Frame, area: Rect) {
    frame.render_widget(&self.0, area);
  }
}

use std::io::{self, Stdout};

use crossterm::terminal::{disable_raw_mode, enable_raw_mode, size};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend};

pub struct TerminalSession {
  pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
  pub fn enter() -> io::Result<Self> {
    enable_raw_mode()?;
    let viewport_height = size().map(|(_, height)| height).unwrap_or(24).max(1);
    let options = TerminalOptions {
      viewport: Viewport::Inline(viewport_height),
    };
    match Terminal::with_options(CrosstermBackend::new(io::stdout()), options) {
      Ok(terminal) => Ok(Self { terminal }),
      Err(error) => {
        let _ = disable_raw_mode();
        Err(error)
      }
    }
  }
}

impl Drop for TerminalSession {
  fn drop(&mut self) {
    let _ = disable_raw_mode();
    let _ = self.terminal.show_cursor();
  }
}

use std::io::{self, Stdout};

use crossterm::{
  execute,
  terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub struct TerminalSession {
  pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
  pub fn enter() -> io::Result<Self> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
      let _ = disable_raw_mode();
      return Err(error);
    }
    match Terminal::new(CrosstermBackend::new(stdout)) {
      Ok(terminal) => Ok(Self { terminal }),
      Err(error) => {
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        Err(error)
      }
    }
  }
}

impl Drop for TerminalSession {
  fn drop(&mut self) {
    let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = self.terminal.show_cursor();
  }
}

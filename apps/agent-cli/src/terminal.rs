use std::{
  io,
  ops::{Deref, DerefMut},
};

use ratatui::DefaultTerminal;

pub struct TerminalGuard(DefaultTerminal);

impl TerminalGuard {
  pub fn enter() -> io::Result<Self> {
    match ratatui::try_init() {
      Ok(terminal) => {
        if let Err(error) =
          crossterm::execute!(io::stdout(), crossterm::event::EnableBracketedPaste)
        {
          ratatui::restore();
          return Err(error);
        }
        Ok(Self(terminal))
      }
      Err(error) => {
        ratatui::restore();
        Err(error)
      }
    }
  }
}

impl Deref for TerminalGuard {
  type Target = DefaultTerminal;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for TerminalGuard {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

impl Drop for TerminalGuard {
  fn drop(&mut self) {
    let _ = crossterm::execute!(io::stdout(), crossterm::event::DisableBracketedPaste);
    ratatui::restore();
  }
}

pub async fn shutdown() {
  #[cfg(unix)]
  {
    if let Ok(mut terminate) =
      tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
      tokio::select! {
          _ = tokio::signal::ctrl_c() => {},
          _ = terminate.recv() => {},
      }
      return;
    }
  }
  let _ = tokio::signal::ctrl_c().await;
}

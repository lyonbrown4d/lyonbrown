use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::Locale;

use super::session::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageCommand {
  NotLanguage,
  Toggle,
  Set(Locale),
  Invalid,
}

pub fn language_toggle_key(key: KeyEvent) -> bool {
  key.code == KeyCode::F(2) && key.modifiers == KeyModifiers::NONE
}

pub(super) fn apply(runtime: &mut Runtime, command: LanguageCommand) -> bool {
  match command {
    LanguageCommand::NotLanguage => {
      runtime.state.clear_client_local_notice();
      return false;
    }
    LanguageCommand::Toggle => {
      runtime.locale = runtime.locale.toggled();
      runtime.state.clear_client_local_notice();
    }
    LanguageCommand::Set(locale) => {
      runtime.locale = locale;
      runtime.state.clear_client_local_notice();
    }
    LanguageCommand::Invalid => runtime
      .state
      .set_client_local_notice(runtime.locale.messages().palette.language_usage),
  }
  true
}

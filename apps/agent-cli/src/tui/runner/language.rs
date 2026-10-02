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

pub fn language_command(input: &str) -> LanguageCommand {
  let mut parts = input.split_whitespace();
  if parts.next() != Some("/language") {
    return LanguageCommand::NotLanguage;
  }
  let locale = match parts.next() {
    None => return LanguageCommand::Toggle,
    Some("en-US") => Locale::EnUs,
    Some("zh-CN") => Locale::ZhCn,
    Some(_) => return LanguageCommand::Invalid,
  };
  if parts.next().is_some() {
    LanguageCommand::Invalid
  } else {
    LanguageCommand::Set(locale)
  }
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
      .set_client_local_notice(language_usage(runtime.locale)),
  }
  true
}

fn language_usage(locale: Locale) -> &'static str {
  match locale {
    Locale::EnUs => "usage: /language [en-US|zh-CN]",
    Locale::ZhCn => "用法：/language [en-US|zh-CN]",
  }
}

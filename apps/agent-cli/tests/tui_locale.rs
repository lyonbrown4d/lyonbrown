use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use agent_cli::tui::{LanguageCommand, Locale, language_command, language_toggle_key};

#[test]
fn system_locale_tags_select_chinese_or_fall_back_to_english() {
  for tag in ["zh-CN", "zh_CN", "zh-Hans", "zh-Hans-CN", "ZH_cn.UTF-8"] {
    assert_eq!(Locale::from_system_tag(Some(tag)), Locale::ZhCn, "{tag}");
  }
  for tag in [
    None,
    Some(""),
    Some("en-US"),
    Some("fr-FR"),
    Some("zh-Hant-TW"),
  ] {
    assert_eq!(Locale::from_system_tag(tag), Locale::EnUs, "{tag:?}");
  }
}

#[test]
fn locale_toggle_round_trips_between_supported_languages() {
  assert_eq!(Locale::EnUs.toggled(), Locale::ZhCn);
  assert_eq!(Locale::ZhCn.toggled(), Locale::EnUs);
}

#[test]
fn f2_is_the_only_global_language_toggle_key() {
  assert!(language_toggle_key(KeyEvent::new(
    KeyCode::F(2),
    KeyModifiers::NONE
  )));
  assert!(!language_toggle_key(KeyEvent::new(
    KeyCode::F(2),
    KeyModifiers::CONTROL
  )));
  assert!(!language_toggle_key(KeyEvent::new(
    KeyCode::F(3),
    KeyModifiers::NONE
  )));
}

#[test]
fn language_command_parses_toggle_and_explicit_supported_locales() {
  assert_eq!(language_command("/language"), LanguageCommand::Toggle);
  assert_eq!(
    language_command("  /language en-US  "),
    LanguageCommand::Set(Locale::EnUs)
  );
  assert_eq!(
    language_command("/language zh-CN"),
    LanguageCommand::Set(Locale::ZhCn)
  );
}

#[test]
fn language_command_rejects_extra_or_unknown_arguments_without_claiming_other_input() {
  assert_eq!(language_command("hello"), LanguageCommand::NotLanguage);
  assert_eq!(
    language_command("/language fr-FR"),
    LanguageCommand::Invalid
  );
  assert_eq!(
    language_command("/language en-US extra"),
    LanguageCommand::Invalid
  );
}

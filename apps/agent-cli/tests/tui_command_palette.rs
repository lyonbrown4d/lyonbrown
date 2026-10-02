use agent_app::{RuntimePhase, RuntimeSnapshot};
use agent_cli::tui::runner::{LocalCommandResult, Runtime};
use agent_cli::tui::{
  CommandArgument, CommandId, CommandInvocation, Locale, OverlayState, SlashOutcome, TuiState,
  UiIntent, command_catalog, slash_command, ui_intent,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::watch;

#[test]
fn catalog_has_stable_local_and_disabled_daemon_commands() {
  let commands = command_catalog();

  assert!(commands.iter().any(|command| command.id == CommandId::Help));
  assert!(
    commands
      .iter()
      .any(|command| command.id == CommandId::Language)
  );
  for id in [
    CommandId::Model,
    CommandId::Sessions,
    CommandId::New,
    CommandId::Resume,
    CommandId::Status,
  ] {
    let command = commands.iter().find(|command| command.id == id).unwrap();
    assert!(command.daemon_backed);
    assert!(!command.implemented);
  }
}

#[test]
fn shortcuts_are_exact_and_question_mark_requires_an_empty_composer() {
  assert_eq!(
    ui_intent(
      KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
      true,
      false,
    ),
    UiIntent::OpenPalette
  );
  assert_eq!(
    ui_intent(
      KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
      ),
      true,
      false,
    ),
    UiIntent::None
  );
  assert_eq!(
    ui_intent(
      KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
      true,
      false
    ),
    UiIntent::OpenHelp
  );
  assert_eq!(
    ui_intent(
      KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
      false,
      false
    ),
    UiIntent::None
  );
  assert_eq!(
    ui_intent(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), false, true),
    UiIntent::CloseOverlay
  );
  assert_eq!(
    ui_intent(
      KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
      false,
      true,
    ),
    UiIntent::CloseOverlay
  );
}

#[test]
fn slash_commands_never_treat_unknown_or_malformed_input_as_a_prompt() {
  assert_eq!(slash_command("hello"), SlashOutcome::NotSlash);
  assert_eq!(
    slash_command("/help"),
    SlashOutcome::Command(CommandInvocation::without_arguments(CommandId::Help))
  );
  assert_eq!(
    slash_command("/help now"),
    SlashOutcome::Invalid(CommandId::Help)
  );
  assert_eq!(slash_command("/wat"), SlashOutcome::Unknown("/wat".into()));
  assert_eq!(
    slash_command("/queue"),
    SlashOutcome::Invalid(CommandId::Queue)
  );
  assert_eq!(
    slash_command("/queue inspect tests"),
    SlashOutcome::Command(CommandInvocation {
      id: CommandId::Queue,
      argument: CommandArgument::Text("inspect tests".into()),
    })
  );
}

#[test]
fn overlay_survives_locale_changes_and_preserves_composer_when_closed() {
  let mut runtime = runtime(Locale::EnUs);
  runtime.state.apply_input_key(KeyCode::Char('d'));
  runtime.state.open_palette();
  runtime
    .state
    .apply_overlay_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE), 16);

  runtime.apply_language_command(agent_cli::tui::LanguageCommand::Toggle);
  assert_eq!(runtime.locale(), Locale::ZhCn);
  assert!(matches!(runtime.state.overlay(), OverlayState::Palette { query, .. } if query == "h"));
  let before = runtime.state.input().to_owned();
  runtime
    .state
    .apply_overlay_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 16);

  assert_eq!(runtime.state.input(), before);
  assert_eq!(runtime.state.overlay(), &OverlayState::Closed);
}

#[test]
fn palette_query_replaces_terminal_and_direction_controls() {
  let mut state = ready_state();
  state.open_palette();

  state.apply_overlay_key(
    KeyEvent::new(KeyCode::Char('\u{202e}'), KeyModifiers::NONE),
    16,
  );

  assert!(matches!(state.overlay(), OverlayState::Palette { query, .. } if query == "�"));
}

#[test]
fn control_p_switches_help_to_the_command_palette() {
  let mut state = ready_state();
  state.open_help();

  state.apply_overlay_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL), 16);

  assert!(matches!(state.overlay(), OverlayState::Palette { .. }));
}

#[test]
fn local_slash_handling_opens_help_localizes_errors_and_never_returns_a_prompt() {
  for (locale, unknown, disabled) in [
    (Locale::EnUs, "Unknown command", "not implemented"),
    (Locale::ZhCn, "未知命令", "尚未实现"),
  ] {
    let mut runtime = runtime(locale);
    assert_eq!(
      runtime.apply_slash_input("/wat"),
      LocalCommandResult::Handled
    );
    assert!(
      runtime
        .state
        .client_local_notice()
        .unwrap()
        .contains(unknown)
    );
    assert_eq!(
      runtime.apply_slash_input("/help extra"),
      LocalCommandResult::Handled
    );
    assert!(
      runtime
        .state
        .client_local_notice()
        .unwrap()
        .contains("/help")
    );
    assert_eq!(
      runtime.apply_slash_input("/sessions"),
      LocalCommandResult::Handled
    );
    assert!(
      runtime
        .state
        .client_local_notice()
        .unwrap()
        .contains(disabled)
    );
    assert_eq!(
      runtime.apply_slash_input("/help"),
      LocalCommandResult::Handled
    );
    assert!(matches!(runtime.state.overlay(), OverlayState::Help { .. }));
  }
}

#[test]
fn local_command_results_keep_queue_and_quit_explicit() {
  let mut runtime = runtime(Locale::EnUs);
  assert_eq!(
    runtime.apply_slash_input("hello"),
    LocalCommandResult::NotCommand
  );
  assert_eq!(
    runtime.apply_slash_input("/queue next task"),
    LocalCommandResult::Queue("next task".into())
  );
  assert_eq!(runtime.apply_slash_input("/quit"), LocalCommandResult::Exit);
}

fn ready_state() -> TuiState {
  let mut state = TuiState::default();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    ..RuntimeSnapshot::default()
  });
  state
}

fn runtime(locale: Locale) -> Runtime {
  let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
  Runtime {
    state: ready_state(),
    client: None,
    snapshots,
    remote_state: None,
    approval: None,
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
    locale,
  }
}

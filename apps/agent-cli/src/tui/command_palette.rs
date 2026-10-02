use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{ActivityInspectorState, Locale};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
  Model,
  Theme,
  Language,
  Sessions,
  New,
  Resume,
  Status,
  Queue,
  Help,
  Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandArgumentKind {
  None,
  OptionalLocale,
  RequiredText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
  pub id: CommandId,
  pub alias: &'static str,
  pub display: &'static str,
  pub daemon_backed: bool,
  pub implemented: bool,
  pub arguments: CommandArgumentKind,
}

const COMMANDS: &[CommandSpec] = &[
  command(CommandId::Model, "/model", "/model", true, false),
  command(CommandId::Theme, "/theme", "/theme", false, false),
  CommandSpec {
    id: CommandId::Language,
    alias: "/language",
    display: "/language [en-US|zh-CN]",
    daemon_backed: false,
    implemented: true,
    arguments: CommandArgumentKind::OptionalLocale,
  },
  command(CommandId::Sessions, "/sessions", "/sessions", true, false),
  command(CommandId::New, "/new", "/new", true, false),
  command(CommandId::Resume, "/resume", "/resume", true, false),
  command(CommandId::Status, "/status", "/status", true, false),
  CommandSpec {
    id: CommandId::Queue,
    alias: "/queue",
    display: "/queue <message>",
    daemon_backed: true,
    implemented: true,
    arguments: CommandArgumentKind::RequiredText,
  },
  command(CommandId::Help, "/help", "/help", false, true),
  command(CommandId::Quit, "/quit", "/quit", false, true),
];

const fn command(
  id: CommandId,
  alias: &'static str,
  display: &'static str,
  daemon_backed: bool,
  implemented: bool,
) -> CommandSpec {
  CommandSpec {
    id,
    alias,
    display,
    daemon_backed,
    implemented,
    arguments: CommandArgumentKind::None,
  }
}

pub fn command_catalog() -> &'static [CommandSpec] {
  COMMANDS
}

pub fn command_by_id(id: CommandId) -> &'static CommandSpec {
  COMMANDS
    .iter()
    .find(|command| command.id == id)
    .expect("every CommandId must have one catalog entry")
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum OverlayState {
  #[default]
  Closed,
  Palette {
    query: String,
    selected: usize,
  },
  Help {
    scroll: u16,
  },
  Activity(ActivityInspectorState),
}

impl OverlayState {
  pub const fn is_open(&self) -> bool {
    !matches!(self, Self::Closed)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiIntent {
  None,
  OpenPalette,
  OpenHelp,
  ToggleActivity,
  CloseOverlay,
  Activate(CommandId),
}

pub fn ui_intent(key: KeyEvent, composer_empty: bool, overlay_open: bool) -> UiIntent {
  if overlay_open
    && (key.code == KeyCode::Esc
      || (key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL))
  {
    return UiIntent::CloseOverlay;
  }
  if key.code == KeyCode::Char('o') && key.modifiers == KeyModifiers::CONTROL {
    return UiIntent::ToggleActivity;
  }
  if key.code == KeyCode::Char('p') && key.modifiers == KeyModifiers::CONTROL {
    return UiIntent::OpenPalette;
  }
  if !overlay_open
    && composer_empty
    && key.code == KeyCode::Char('?')
    && key.modifiers == KeyModifiers::NONE
  {
    return UiIntent::OpenHelp;
  }
  UiIntent::None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandArgument {
  None,
  Locale(Option<Locale>),
  Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInvocation {
  pub id: CommandId,
  pub argument: CommandArgument,
}

impl CommandInvocation {
  pub const fn without_arguments(id: CommandId) -> Self {
    Self {
      id,
      argument: CommandArgument::None,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashOutcome {
  NotSlash,
  Command(CommandInvocation),
  Invalid(CommandId),
  Unknown(String),
}

impl SlashOutcome {
  pub const fn command_id(&self) -> Option<CommandId> {
    match self {
      Self::Command(invocation) => Some(invocation.id),
      Self::Invalid(id) => Some(*id),
      Self::NotSlash | Self::Unknown(_) => None,
    }
  }
}

pub fn slash_command(input: &str) -> SlashOutcome {
  let trimmed = input.trim();
  if !trimmed.starts_with('/') {
    return SlashOutcome::NotSlash;
  }
  let alias = trimmed.split_whitespace().next().unwrap_or_default();
  let Some(spec) = COMMANDS.iter().find(|command| command.alias == alias) else {
    return SlashOutcome::Unknown(alias.to_owned());
  };
  let remainder = trimmed.strip_prefix(spec.alias).unwrap_or_default().trim();
  let argument = match spec.arguments {
    CommandArgumentKind::None if remainder.is_empty() => CommandArgument::None,
    CommandArgumentKind::None => return SlashOutcome::Invalid(spec.id),
    CommandArgumentKind::RequiredText if remainder.is_empty() => {
      return SlashOutcome::Invalid(spec.id);
    }
    CommandArgumentKind::RequiredText => CommandArgument::Text(remainder.to_owned()),
    CommandArgumentKind::OptionalLocale => match remainder {
      "" => CommandArgument::Locale(None),
      "en-US" => CommandArgument::Locale(Some(Locale::EnUs)),
      "zh-CN" => CommandArgument::Locale(Some(Locale::ZhCn)),
      _ => return SlashOutcome::Invalid(spec.id),
    },
  };
  SlashOutcome::Command(CommandInvocation {
    id: spec.id,
    argument,
  })
}

pub(crate) fn matching_commands(query: &str) -> Vec<&'static CommandSpec> {
  let query = query.trim().to_ascii_lowercase();
  COMMANDS
    .iter()
    .filter(|command| {
      query.is_empty()
        || command
          .alias
          .trim_start_matches('/')
          .to_ascii_lowercase()
          .contains(&query)
    })
    .collect()
}

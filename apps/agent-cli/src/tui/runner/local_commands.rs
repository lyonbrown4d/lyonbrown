use crate::tui::{
  CommandArgument, CommandId, CommandInvocation, SlashOutcome, command_by_id, slash_command,
};

use super::{LanguageCommand, Runtime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalCommandResult {
  NotCommand,
  Handled,
  Exit,
  Queue(String),
}

impl Runtime {
  pub(super) fn set_daemon_required_notice(&mut self) {
    self
      .state
      .set_client_local_notice(self.locale.messages().palette.daemon_required);
  }

  pub fn apply_slash_input(&mut self, input: &str) -> LocalCommandResult {
    match slash_command(input) {
      SlashOutcome::NotSlash => {
        self.state.clear_client_local_notice();
        LocalCommandResult::NotCommand
      }
      SlashOutcome::Command(invocation) => self.apply_command_invocation(invocation),
      SlashOutcome::Invalid(command) => {
        let messages = self.locale.messages().palette;
        self
          .state
          .set_client_local_notice(command_usage(command, messages));
        LocalCommandResult::Handled
      }
      SlashOutcome::Unknown(command) => {
        let messages = self.locale.messages().palette;
        let notice = format!(
          "{}: {command} · {}",
          messages.unknown_command, messages.command_hint
        );
        self.state.set_client_local_notice(notice);
        LocalCommandResult::Handled
      }
    }
  }

  pub fn apply_command_id(&mut self, command: CommandId) -> LocalCommandResult {
    let argument = match command {
      CommandId::Language => CommandArgument::Locale(None),
      _ => CommandArgument::None,
    };
    self.apply_command_invocation(CommandInvocation {
      id: command,
      argument,
    })
  }

  fn apply_command_invocation(&mut self, invocation: CommandInvocation) -> LocalCommandResult {
    let spec = command_by_id(invocation.id);
    if !spec.implemented {
      self
        .state
        .set_client_local_notice(self.locale.messages().palette.command_not_implemented);
      return LocalCommandResult::Handled;
    }
    match (invocation.id, invocation.argument) {
      (CommandId::Help, CommandArgument::None) => {
        self.state.clear_client_local_notice();
        self.state.open_help();
        LocalCommandResult::Handled
      }
      (CommandId::Language, CommandArgument::Locale(locale)) => {
        self.apply_language_command(locale.map_or(LanguageCommand::Toggle, LanguageCommand::Set));
        LocalCommandResult::Handled
      }
      (CommandId::Quit, CommandArgument::None) => LocalCommandResult::Exit,
      (CommandId::Queue, CommandArgument::Text(message)) => {
        self.state.clear_client_local_notice();
        LocalCommandResult::Queue(message)
      }
      (command, _) => {
        let messages = self.locale.messages().palette;
        self
          .state
          .set_client_local_notice(command_usage(command, messages));
        LocalCommandResult::Handled
      }
    }
  }
}

pub fn language_command(input: &str) -> LanguageCommand {
  match slash_command(input) {
    SlashOutcome::Command(CommandInvocation {
      id: CommandId::Language,
      argument: CommandArgument::Locale(locale),
    }) => locale.map_or(LanguageCommand::Toggle, LanguageCommand::Set),
    SlashOutcome::Invalid(CommandId::Language) => LanguageCommand::Invalid,
    _ => LanguageCommand::NotLanguage,
  }
}

fn command_usage(
  command: CommandId,
  messages: crate::tui::locale::PaletteMessages,
) -> &'static str {
  match command {
    CommandId::Model => messages.model_usage,
    CommandId::Theme => messages.theme_usage,
    CommandId::Language => messages.language_usage,
    CommandId::Sessions => messages.sessions_usage,
    CommandId::New => messages.new_usage,
    CommandId::Resume => messages.resume_usage,
    CommandId::Status => messages.status_usage,
    CommandId::Queue => messages.queue_usage,
    CommandId::Help => messages.help_usage,
    CommandId::Quit => messages.quit_usage,
  }
}

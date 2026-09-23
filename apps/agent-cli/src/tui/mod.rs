mod approval;
mod composer;
mod input;
pub mod runner;
mod state;
mod terminal;
mod ui;

pub use approval::{ApprovalRequest, TuiApprovalPrompt};
pub use runner::run;
pub use state::{Speaker, TranscriptEntry, TuiState};
pub use ui::{render, render_with_approval, render_with_scroll};

pub(crate) fn terminal_safe(value: &str) -> String {
  value
    .chars()
    .map(|character| match character {
      '\n' | '\t' => character,
      _ if character.is_control() => '\u{fffd}',
      _ => character,
    })
    .collect()
}

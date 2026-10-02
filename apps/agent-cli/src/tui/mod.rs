mod approval;
mod composer;
mod connection;
mod input;
mod locale;
pub mod runner;
mod state;
mod terminal;
mod theme;
mod ui;

pub use approval::ApprovalRequest;
pub(crate) use approval::approval_safe;
pub use connection::TuiConnectionState;
pub use connection::apply_remote_state;
pub use locale::Locale;
pub use runner::{run_client, run_native_client};
pub use state::{Speaker, TranscriptEntry, TuiState};
pub use theme::Theme;
pub use ui::{
  RenderContext, render, render_with_approval, render_with_context, render_with_scroll,
};

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

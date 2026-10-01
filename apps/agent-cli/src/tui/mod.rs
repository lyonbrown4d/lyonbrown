mod approval;
mod composer;
mod embedded;
mod input;
mod locale;
mod model_setup;
mod model_setup_runner;
pub mod runner;
mod state;
mod terminal;
mod theme;
mod ui;

pub use approval::ApprovalRequest;
pub(crate) use approval::approval_safe;
pub use embedded::run_embedded;
pub use embedded::run_embedded as run;
pub use locale::Locale;
pub use model_setup::{ModelSetupView, render_model_setup};
pub use model_setup_runner::prepare_embedded_model;
pub use runner::run_client;
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

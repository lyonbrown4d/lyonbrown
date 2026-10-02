mod approval;
mod composer;
mod connection;
mod input;
mod locale;
mod model_setup;
pub mod runner;
mod state;
mod terminal;
mod text_safety;
mod theme;
mod ui;

pub use approval::ApprovalRequest;
pub(crate) use approval::approval_safe;
pub use connection::TuiConnectionState;
pub use connection::apply_remote_state;
pub use locale::Locale;
#[doc(hidden)]
pub use model_setup::format_model_setup_progress;
#[doc(hidden)]
pub use runner::{LanguageCommand, language_command, language_toggle_key};
pub use runner::{run_client, run_native_client};
pub use state::{Speaker, TranscriptEntry, TuiState};
#[doc(hidden)]
pub use text_safety::{status_line_safe, transcript_safe};
pub use theme::Theme;
pub use ui::{
  RenderContext, render, render_with_approval, render_with_context, render_with_scroll,
};

pub(crate) fn terminal_safe(value: &str) -> String {
  transcript_safe(value)
}

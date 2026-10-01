use agent_cli::model_manager::{DownloadPhase, ModelDownloadProgress};
use agent_cli::tui::{Locale, ModelSetupView, Theme, render_model_setup};
use ratatui::{Terminal, backend::TestBackend};

fn setup_screen(
  locale: Locale,
  progress: ModelDownloadProgress,
  awaiting_confirmation: bool,
) -> String {
  let backend = TestBackend::new(72, 14);
  let mut terminal = Terminal::new(backend).unwrap();
  let view = ModelSetupView {
    model_id: "qwen3-1.7b-q4_k_m",
    download_bytes: 1_110_000_000,
    progress,
    awaiting_confirmation,
  };
  terminal
    .draw(|frame| render_model_setup(frame, &view, Theme::default(), locale))
    .unwrap();
  let buffer = terminal.backend().buffer();
  (0..14)
    .map(|y| {
      (0..72)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn downloading_model_renders_a_progress_bar_and_cancel_hint() {
  let output = setup_screen(
    Locale::EnUs,
    ModelDownloadProgress::new(DownloadPhase::Downloading, 555_000_000, Some(1_110_000_000)),
    false,
  );

  assert!(output.contains("qwen3-1.7b-q4_k_m"));
  assert!(output.contains("50%"));
  assert!(output.contains("cancel"));
}

#[test]
fn model_download_confirmation_is_localized_and_optional() {
  let output = setup_screen(
    Locale::ZhCn,
    ModelDownloadProgress::new(DownloadPhase::Checking, 0, Some(1_110_000_000)),
    true,
  );
  let compact = output.replace(' ', "");

  assert!(compact.contains("下载"), "{output}");
  assert!(compact.contains("不下载并退出"), "{output}");
}

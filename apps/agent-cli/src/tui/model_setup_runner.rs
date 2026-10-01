use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::model_manager::{
  DownloadPhase, ModelDownloadProgress, PreparedModel, available_memory_bytes, cached_model,
  ensure_model_from_endpoint_with_progress, requested_preset,
};

use super::{
  Locale, ModelSetupView, Theme, model_setup::render_model_setup, terminal::TerminalSession,
};

pub async fn prepare_embedded_model(
  home: &Path,
  requested: Option<&str>,
) -> Result<Option<PreparedModel>> {
  let preset = requested_preset(requested)?;
  ensure_memory_available(preset.min_available_memory_bytes, preset.id)?;
  let cache = home.join(".agent").join("models");
  let locale = selected_locale();
  let theme = Theme::default();
  let mut terminal = TerminalSession::enter().context("failed to initialize model setup TUI")?;
  let checking =
    ModelDownloadProgress::new(DownloadPhase::Checking, 0, Some(preset.download_bytes));
  draw(
    &mut terminal,
    preset.id,
    preset.download_bytes,
    checking,
    false,
    theme,
    locale,
  )?;

  if let Some(path) = cached_model(&cache, preset).await? {
    return Ok(Some(PreparedModel {
      id: preset.id,
      path,
    }));
  }
  if !confirm_download(
    &mut terminal,
    preset.id,
    preset.download_bytes,
    theme,
    locale,
  )? {
    return Ok(None);
  }
  download_model(&mut terminal, cache, preset, theme, locale).await
}

async fn download_model(
  terminal: &mut TerminalSession,
  cache: PathBuf,
  preset: &'static crate::model_manager::ModelPreset,
  theme: Theme,
  locale: Locale,
) -> Result<Option<PreparedModel>> {
  let initial = ModelDownloadProgress::new(DownloadPhase::Checking, 0, Some(preset.download_bytes));
  let (updates, progress) = watch::channel(initial);
  let cancellation = CancellationToken::new();
  let task_cancellation = cancellation.clone();
  let task_cache = cache.clone();
  let mut task = tokio::spawn(async move {
    ensure_model_from_endpoint_with_progress(
      &task_cache,
      preset,
      "https://huggingface.co",
      updates,
      task_cancellation,
    )
    .await
  });

  loop {
    if let Err(error) = draw(
      terminal,
      preset.id,
      preset.download_bytes,
      progress.borrow().clone(),
      false,
      theme,
      locale,
    ) {
      cancellation.cancel();
      let _ = task.await;
      return Err(error);
    }
    if task.is_finished() {
      let path = task.await.context("model download task failed")??;
      return Ok(Some(PreparedModel {
        id: preset.id,
        path,
      }));
    }
    let cancel_requested = match download_cancel_requested() {
      Ok(cancel_requested) => cancel_requested,
      Err(error) => {
        cancellation.cancel();
        let _ = task.await;
        return Err(error);
      }
    };
    if cancel_requested {
      cancellation.cancel();
      match (&mut task).await {
        Ok(Err(error)) if error.to_string().contains("cancelled") => return Ok(None),
        Ok(Err(error)) => return Err(error),
        Err(error) => return Err(error).context("model download task failed"),
        Ok(Ok(_)) => {
          return Ok(Some(PreparedModel {
            id: preset.id,
            path: cache.join(preset.filename),
          }));
        }
      }
    }
  }
}

fn download_cancel_requested() -> Result<bool> {
  if !event::poll(Duration::from_millis(50))? {
    return Ok(false);
  }
  let Event::Key(key) = event::read()? else {
    return Ok(false);
  };
  Ok(
    key.kind == KeyEventKind::Press
      && matches!(
        key.code,
        KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C')
      ),
  )
}

fn confirm_download(
  terminal: &mut TerminalSession,
  model_id: &str,
  download_bytes: u64,
  theme: Theme,
  locale: Locale,
) -> Result<bool> {
  loop {
    draw(
      terminal,
      model_id,
      download_bytes,
      ModelDownloadProgress::new(DownloadPhase::Checking, 0, Some(download_bytes)),
      true,
      theme,
      locale,
    )?;
    if event::poll(Duration::from_millis(50))?
      && let Event::Key(key) = event::read()?
      && key.kind == KeyEventKind::Press
    {
      match key.code {
        KeyCode::Enter | KeyCode::Char('d') | KeyCode::Char('D') => return Ok(true),
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(false),
        _ => {}
      }
    }
  }
}

fn draw(
  terminal: &mut TerminalSession,
  model_id: &str,
  download_bytes: u64,
  progress: ModelDownloadProgress,
  awaiting_confirmation: bool,
  theme: Theme,
  locale: Locale,
) -> Result<()> {
  let view = ModelSetupView {
    model_id,
    download_bytes,
    progress,
    awaiting_confirmation,
  };
  terminal
    .terminal
    .draw(|frame| render_model_setup(frame, &view, theme, locale))?;
  Ok(())
}

fn selected_locale() -> Locale {
  std::env::var("LYONBROWN_UI_LOCALE")
    .ok()
    .map(|value| Locale::from_id(&value))
    .unwrap_or_default()
}

fn ensure_memory_available(required: u64, model_id: &str) -> Result<()> {
  let available = available_memory_bytes();
  if available < required {
    bail!(
      "model preset '{model_id}' needs at least {:.1} GiB available RAM; found {:.1} GiB",
      required as f64 / (1024_u64.pow(3)) as f64,
      available as f64 / (1024_u64.pow(3)) as f64
    );
  }
  Ok(())
}

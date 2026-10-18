//! Shared persistent structured logging for Lyonbrown processes.
use std::{
  fs,
  path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use tracing_appender::{
  non_blocking::{ErrorCounter, NonBlockingBuilder, WorkerGuard},
  rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt};

#[derive(Clone, Debug)]
pub struct LogOptions {
  pub directory: PathBuf,
  pub retained_files: usize,
  pub stderr: bool,
}

impl LogOptions {
  pub fn for_data_dir(data_dir: &Path) -> Self {
    Self {
      directory: data_dir.join("logs"),
      retained_files: 14,
      stderr: false,
    }
  }
}

/// Keep this guard in the process entrypoint until other logging producers stop.
pub struct LogGuard {
  _writer: WorkerGuard,
  dropped: ErrorCounter,
}

impl LogGuard {
  pub fn dropped_lines(&self) -> usize {
    self.dropped.dropped_lines()
  }
}

pub fn init(data_dir: &Path, process_name: &str, filter: &str) -> Result<LogGuard> {
  init_with_options(&LogOptions::for_data_dir(data_dir), process_name, filter)
}

pub fn init_with_options(
  options: &LogOptions,
  process_name: &str,
  filter: &str,
) -> Result<LogGuard> {
  let (dispatch, guard) = build_dispatch(options, process_name, filter)?;
  tracing::dispatcher::set_global_default(dispatch)
    .context("install process tracing subscriber")?;
  Ok(guard)
}

/// Build an isolated subscriber for embedding or deterministic tests.
pub fn build_dispatch(
  options: &LogOptions,
  process_name: &str,
  filter: &str,
) -> Result<(tracing::Dispatch, LogGuard)> {
  ensure!(
    !process_name.is_empty()
      && process_name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
    "log process name must contain only ASCII letters, digits, hyphens or underscores"
  );
  ensure!(
    (1..=365).contains(&options.retained_files),
    "log retention must be between 1 and 365 files"
  );
  ensure!(
    options.directory.is_absolute(),
    "log directory must be absolute"
  );
  let filter = EnvFilter::try_new(filter).context("invalid log filter")?;
  fs::create_dir_all(&options.directory).context("create private log directory")?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&options.directory, fs::Permissions::from_mode(0o700))?;
  }
  let file = RollingFileAppender::builder()
    .rotation(Rotation::DAILY)
    .filename_prefix(process_name)
    .filename_suffix("jsonl")
    .max_log_files(options.retained_files)
    .build(&options.directory)
    .context("open rolling log file")?;
  let (writer, guard) = NonBlockingBuilder::default()
    .buffered_lines_limit(8192)
    .lossy(true)
    .thread_name(&format!("{process_name}-log-writer"))
    .finish(file);
  let dropped = writer.error_counter();
  let file_layer = tracing_subscriber::fmt::layer()
    .json()
    .with_ansi(false)
    .with_current_span(true)
    .with_span_list(true)
    .with_writer(writer);
  let stderr_layer = options.stderr.then(|| {
    tracing_subscriber::fmt::layer()
      .with_ansi(false)
      .with_writer(std::io::stderr)
  });
  let subscriber = tracing_subscriber::registry()
    .with(filter)
    .with(file_layer)
    .with(stderr_layer);
  Ok((
    tracing::Dispatch::new(subscriber),
    LogGuard {
      _writer: guard,
      dropped,
    },
  ))
}

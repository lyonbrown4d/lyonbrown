use std::{io::IsTerminal as _, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};

pub(crate) const MAX_PROMPT_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OutputFormat {
  #[default]
  Text,
  Jsonl,
}

#[derive(Args)]
pub struct ExecArgs {
  /// Prompt text. Use '-' to explicitly read stdin; piped stdin is appended to text.
  pub prompt: Option<String>,
  /// Existing daemon session; uses its workspace and current owner epoch.
  #[arg(long, conflicts_with = "workspace", value_parser = crate::task_command::identifier)]
  pub session: Option<String>,
  /// Workspace for a new session; defaults to the current directory.
  #[arg(long)]
  pub workspace: Option<PathBuf>,
  #[arg(long, value_enum, default_value = "text")]
  pub output_format: OutputFormat,
  /// Total CLI deadline in seconds, including stdin and connection setup.
  #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=86400))]
  pub timeout: u64,
  /// Submission identity; never automatically retried after an unknown response.
  #[arg(long, value_parser = crate::task_command::identifier)]
  pub idempotency_key: Option<String>,
}

pub(crate) async fn read_prompt(argument: Option<&str>) -> Result<String> {
  let explicit = argument == Some("-");
  let mut prompt = argument
    .filter(|_| !explicit)
    .unwrap_or_default()
    .to_owned();
  ensure!(prompt.len() <= MAX_PROMPT_BYTES, "prompt exceeds 1 MiB");
  if explicit || !std::io::stdin().is_terminal() {
    let limit = MAX_PROMPT_BYTES.saturating_sub(prompt.len());
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // Tokio's blocking stdin read cannot be cancelled at runtime shutdown.
    // A detached dedicated reader lets Ctrl+C/deadlines terminate the CLI even
    // when a pipe or interactive '-' input remains open without reaching EOF.
    std::thread::Builder::new()
      .name("exec-stdin".into())
      .spawn(move || {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        let result = std::io::stdin()
          .lock()
          .take((limit + 1) as u64)
          .read_to_end(&mut bytes)
          .map(|_| bytes);
        let _ = sender.send(result);
      })?;
    let bytes = receiver.await.context("stdin reader stopped")??;
    ensure!(bytes.len() <= limit, "prompt and stdin exceed 1 MiB");
    let input = String::from_utf8(bytes).context("stdin must be UTF-8")?;
    if !prompt.is_empty() && !input.is_empty() {
      prompt.push_str("\n\n");
    }
    prompt.push_str(&input);
  }
  ensure!(
    prompt.len() <= MAX_PROMPT_BYTES,
    "prompt and stdin exceed 1 MiB"
  );
  ensure!(!prompt.trim().is_empty(), "provide a prompt or UTF-8 stdin");
  Ok(prompt)
}

//! Cargo wire schemas stay upstream-owned; pipe ownership and cancellation stay local.
use anyhow::{Context, Result};
use cargo_metadata::{Message, TargetKind};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

pub(crate) async fn executable(stdout: impl AsyncRead + Unpin, package: &str) -> Result<PathBuf> {
  let mut lines = BufReader::new(stdout).lines();
  let mut executable = None;
  // Drain through EOF even after finding the executable: Cargo can still fail later.
  while let Some(line) = lines.next_line().await? {
    match serde_json::from_str::<Message>(&line) {
      Ok(Message::CompilerMessage(message)) => {
        if let Some(rendered) = message.message.rendered {
          eprint!("{rendered}");
        }
      }
      Ok(Message::CompilerArtifact(artifact))
        if artifact.target.name == package && artifact.target.kind.contains(&TargetKind::Bin) =>
      {
        if let Some(path) = artifact.executable {
          executable = Some(path.into_std_path_buf());
        }
      }
      _ => {}
    }
  }
  executable.with_context(|| format!("Cargo did not report the {package} executable"))
}

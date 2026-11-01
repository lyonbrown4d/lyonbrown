//! Workspace-relative file access and content-addressed artifacts.
mod artifact;
mod discovery;
mod files;
mod instructions;
pub mod process;
mod watch;
use anyhow::{Context, Result};
pub use artifact::ArtifactStore;
use cap_std::fs::Dir;
pub use discovery::{ContentMatch, DiscoveryReport, FileListing, FileSearch};
pub use files::{FileEntry, RevisionConflict};
pub use instructions::{
  RepositoryInstruction, load_repository_instructions, load_repository_instructions_for_paths,
};
use std::{
  io::Read,
  path::{Component, Path},
  sync::Arc,
};
pub use watch::{WorkspaceChange, WorkspaceWatch};
#[derive(Clone)]
pub struct Workspace {
  dir: Arc<Dir>,
}
impl Workspace {
  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    Ok(Self {
      dir: Arc::new(
        Dir::open_ambient_dir(path, cap_std::ambient_authority())
          .context("open authorized workspace")?,
      ),
    })
  }
  pub fn read(&self, path: &str) -> Result<String> {
    validate_path(path)?;
    let mut bytes = Vec::new();
    self
      .dir
      .open(path)?
      .take(1_048_577)
      .read_to_end(&mut bytes)?;
    anyhow::ensure!(
      bytes.len() <= 1_048_576,
      "file exceeds 1 MiB tool result limit"
    );
    String::from_utf8(bytes).context("file is not UTF-8 text")
  }
  pub fn write(&self, path: &str, text: &str) -> Result<()> {
    validate_path(path)?;
    anyhow::ensure!(text.len() <= 1_048_576, "file content exceeds 1 MiB limit");
    if let Some(parent) = Path::new(path)
      .parent()
      .filter(|p| !p.as_os_str().is_empty())
    {
      self.dir.create_dir_all(parent)?;
    }
    use std::io::Write;
    let parent = Path::new(path).parent().unwrap_or_else(|| Path::new(""));
    let temporary = parent.join(format!(".lyonbrown-{}.tmp", agent_core::new_id()));
    let result = (|| -> Result<()> {
      let mut file = self.dir.open_with(
        &temporary,
        cap_std::fs::OpenOptions::new().write(true).create_new(true),
      )?;
      file.write_all(text.as_bytes())?;
      file.sync_all()?;
      self.dir.rename(&temporary, &self.dir, path)?;
      Ok(())
    })();
    if result.is_err() {
      let _ = self.dir.remove_file(&temporary);
    }
    result?;
    Ok(())
  }
}
fn validate_path(path: &str) -> Result<()> {
  anyhow::ensure!(
    !path.is_empty()
      && Path::new(path)
        .components()
        .all(|p| matches!(p, Component::Normal(_) | Component::CurDir)),
    "path must stay relative to the authorized workspace"
  );
  Ok(())
}

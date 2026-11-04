//! Capability-relative deletion for a reviewed checkpoint reversal.
use crate::{Workspace, validate_path};
use anyhow::{Result, ensure};
use std::path::{Component, Path, PathBuf};

impl Workspace {
  /// Reject aliasing symlinks and non-normal components for reviewed changes.
  /// Capability-relative APIs still enforce containment if another process races.
  pub fn validate_change_path(&self, path: &str) -> Result<()> {
    validate_path(path)?;
    ensure!(
      path.len() <= 4096
        && !path.contains('\\')
        && path
          .split('/')
          .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(path)
          .components()
          .all(|c| matches!(c, Component::Normal(_))),
      "change path must be normalized"
    );
    let mut candidate = PathBuf::new();
    for component in Path::new(path).components() {
      candidate.push(component.as_os_str());
      match self.dir.symlink_metadata(&candidate) {
        Ok(metadata) => ensure!(
          !metadata.file_type().is_symlink(),
          "change paths cannot traverse symlinks"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
        Err(error) => return Err(error.into()),
      }
    }
    Ok(())
  }

  /// Reuse revision-checked replacement while retaining an existing file's mode.
  pub fn write_change_revision(&self, path: &str, content: &str, expected: &str) -> Result<String> {
    self.validate_change_path(path)?;
    self.check_revision(path, expected)?;
    let permissions = if expected.is_empty() {
      None
    } else {
      Some(self.dir.metadata(path)?.permissions())
    };
    let revision = self.write_revision(path, content, expected)?;
    if let Some(permissions) = permissions {
      self.dir.set_permissions(path, permissions)?;
    }
    Ok(revision)
  }

  /// Checks the current image under the host write lock. External editors do not
  /// share that lock, so this is not an OS compare-and-swap transaction.
  pub fn remove_revision(&self, path: &str, expected_revision: &str) -> Result<()> {
    self.validate_change_path(path)?;
    ensure!(
      !expected_revision.is_empty(),
      "deletion requires an existing revision"
    );
    self.check_revision(path, expected_revision)?;
    ensure!(
      self.dir.symlink_metadata(path)?.file_type().is_file(),
      "deletion requires a regular file"
    );
    self.dir.remove_file(path)?;
    Ok(())
  }
}

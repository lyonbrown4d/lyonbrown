use crate::{Workspace, validate_path};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::Path;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileEntry {
  pub path: String,
  pub directory: bool,
  pub size: u64,
}
/// A known pre-write failure: no replacement was attempted.
#[derive(Debug)]
pub struct RevisionConflict;
impl std::fmt::Display for RevisionConflict {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("file revision conflict: file changed since it was read; reload before saving")
  }
}
impl std::error::Error for RevisionConflict {}
impl Workspace {
  /// Validate a Git path without reading its content or requiring a deleted file to exist.
  /// Existing ancestors must resolve inside the capability directory.
  pub fn validate_diff_path(&self, path: &str) -> Result<()> {
    validate_path(path)?;
    let mut candidate = Path::new(path);
    loop {
      match self.dir.metadata(candidate) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
          if candidate == Path::new(".") {
            return Err(error.into());
          }
          candidate = candidate
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        }
        Err(error) => return Err(error.into()),
      }
    }
  }

  pub fn list(&self, directory: &str) -> Result<Vec<FileEntry>> {
    let path = if directory.is_empty() { "." } else { directory };
    validate_path(path)?;
    let directory = self.dir.open_dir(path)?;
    let mut entries = Vec::new();
    for entry in directory.entries()? {
      let entry = entry?;
      let kind = entry.file_type()?;
      if kind.is_symlink() {
        continue;
      }
      let name = entry.file_name().into_string().map_err(|_| {
        anyhow::anyhow!("workspace contains a filename that cannot be represented as UTF-8")
      })?;
      if matches!(name.as_str(), ".git" | "node_modules" | "target") {
        continue;
      }
      let path = if path == "." {
        name
      } else {
        Path::new(path)
          .join(name)
          .to_string_lossy()
          .replace(std::path::MAIN_SEPARATOR, "/")
      };
      entries.push(FileEntry {
        path,
        directory: kind.is_dir(),
        size: if kind.is_file() {
          entry.metadata()?.len()
        } else {
          0
        },
      });
      anyhow::ensure!(
        entries.len() <= 5000,
        "directory exceeds 5000-entry listing limit"
      );
    }
    entries.sort_by(|a, b| b.directory.cmp(&a.directory).then(a.path.cmp(&b.path)));
    Ok(entries)
  }
  pub fn read_revision(&self, path: &str) -> Result<(String, String)> {
    let content = self.read(path)?;
    let revision = hex::encode(Sha256::digest(content.as_bytes()));
    Ok((content, revision))
  }
  pub fn write_revision(&self, path: &str, content: &str, expected: &str) -> Result<String> {
    self.check_revision(path, expected)?;
    self.write(path, content)?;
    Ok(hex::encode(Sha256::digest(content.as_bytes())))
  }
  /// Check under the caller's write lock immediately before replacing content.
  /// External editors do not participate in that lock; this is not an OS CAS.
  pub fn check_revision(&self, path: &str, expected: &str) -> Result<()> {
    validate_path(path)?;
    anyhow::ensure!(
      expected.is_empty()
        || (expected.len() == 64
          && expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
      "expected_revision must be a lowercase SHA-256 revision, or empty for creation"
    );
    let current = match self.dir.metadata(path) {
      Ok(_) => self.read_revision(path)?.1,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
      Err(error) => return Err(error.into()),
    };
    if current != expected {
      return Err(RevisionConflict.into());
    }
    Ok(())
  }
}

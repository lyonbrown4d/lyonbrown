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
impl Workspace {
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
          .replace('\\', "/")
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
    validate_path(path)?;
    let current = match self.dir.metadata(path) {
      Ok(_) => self.read_revision(path)?.1,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
      Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
      current == expected,
      "file changed since it was read; reload before saving"
    );
    self.write(path, content)?;
    Ok(hex::encode(Sha256::digest(content.as_bytes())))
  }
}

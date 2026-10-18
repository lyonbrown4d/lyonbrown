use agent_core::ArtifactRef;
use anyhow::Result;
use cap_std::fs::{Dir, OpenOptions};
use sha2::{Digest, Sha256};
use std::{
  io::{Read, Write},
  path::Path,
  sync::Arc,
};
#[derive(Clone)]
pub struct ArtifactStore {
  dir: Arc<Dir>,
  node: String,
}
impl ArtifactStore {
  pub fn open(path: &Path, node: &str) -> Result<Self> {
    std::fs::create_dir_all(path)?;
    Ok(Self {
      dir: Arc::new(Dir::open_ambient_dir(path, cap_std::ambient_authority())?),
      node: node.into(),
    })
  }
  pub fn put(&self, bytes: &[u8], mime: &str) -> Result<ArtifactRef> {
    anyhow::ensure!(
      bytes.len() <= 64 * 1024 * 1024,
      "artifact exceeds 64 MiB limit"
    );
    let digest = hex::encode(Sha256::digest(bytes));
    let temporary = format!(".artifact-{}.tmp", agent_core::new_id());
    let publish = (|| -> Result<()> {
      let mut file = self
        .dir
        .open_with(&temporary, OpenOptions::new().write(true).create_new(true))?;
      file.write_all(bytes)?;
      file.sync_all()?;
      // Publish only complete content, without replacing an existing digest.
      match self.dir.hard_link(&temporary, &self.dir, &digest) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
          anyhow::ensure!(
            self.read(&digest)? == bytes,
            "existing artifact differs from supplied content"
          );
          Ok(())
        }
        Err(error) => Err(error.into()),
      }
    })();
    let _ = self.dir.remove_file(&temporary);
    publish?;
    Ok(ArtifactRef {
      digest,
      size: bytes.len() as u64,
      mime: mime.into(),
      node_id: self.node.clone(),
    })
  }
  pub fn read(&self, digest: &str) -> Result<Vec<u8>> {
    anyhow::ensure!(
      digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_hexdigit()),
      "invalid artifact digest"
    );
    let mut value = Vec::new();
    self
      .dir
      .open(digest)?
      .take(64 * 1024 * 1024 + 1)
      .read_to_end(&mut value)?;
    anyhow::ensure!(
      value.len() <= 64 * 1024 * 1024,
      "artifact exceeds 64 MiB limit"
    );
    anyhow::ensure!(
      hex::encode(Sha256::digest(&value)) == digest,
      "artifact digest mismatch"
    );
    Ok(value)
  }
}

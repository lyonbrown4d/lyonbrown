use crate::{ArtifactFile, InstallProgress, InstalledModel};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::{
  fs::File,
  io::{Read, Write},
  path::{Path, PathBuf},
};
use tokio::sync::mpsc;

pub(crate) fn validate_id(id: &str) -> Result<()> {
  ensure!(
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
    "invalid installation id"
  );
  Ok(())
}

pub(crate) fn subdir(parent: &Path, name: &str) -> Result<PathBuf> {
  let path = parent.join(name);
  if let Ok(metadata) = std::fs::symlink_metadata(&path) {
    ensure!(
      metadata.is_dir() && !metadata.file_type().is_symlink(),
      "invalid model directory"
    );
  }
  std::fs::create_dir_all(&path)?;
  Ok(path)
}

pub(crate) fn checked_parents(root: &Path, parent: &Path) -> Result<()> {
  let relative = parent.strip_prefix(root)?;
  let mut path = root.to_path_buf();
  for component in relative.components() {
    path = subdir(&path, &component.as_os_str().to_string_lossy())?;
  }
  Ok(())
}

pub(crate) fn verify(path: &Path, expected: &ArtifactFile) -> Result<()> {
  let metadata = std::fs::symlink_metadata(path)?;
  ensure!(
    metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() == expected.bytes,
    "invalid model file"
  );
  if let Some(expected) = &expected.sha256 {
    let mut digest = Sha256::new();
    let mut file = File::open(path)?;
    let mut buffer = [0u8; 65536];
    loop {
      let count = file.read(&mut buffer)?;
      if count == 0 {
        break;
      }
      digest.update(&buffer[..count]);
    }
    ensure!(
      hex::encode(digest.finalize()).eq_ignore_ascii_case(expected),
      "model digest mismatch"
    );
  }
  Ok(())
}

pub(crate) fn read_installed(path: &Path) -> Result<InstalledModel> {
  let directory = std::fs::symlink_metadata(path)?;
  ensure!(
    directory.is_dir() && !directory.file_type().is_symlink(),
    "invalid installed model directory"
  );
  let manifest = path.join("installed.json");
  let metadata = std::fs::symlink_metadata(&manifest)?;
  ensure!(
    !metadata.file_type().is_symlink() && metadata.len() <= 2 * 1024 * 1024,
    "invalid model manifest"
  );
  let mut installed: InstalledModel = serde_json::from_reader(File::open(manifest)?)?;
  validate_id(&installed.plan.id)?;
  ensure!(
    installed.verified_files.len() == installed.plan.files.len(),
    "incomplete verification manifest"
  );
  for file in &installed.verified_files {
    ensure!(file.sha256.is_some(), "missing model verification hash");
    crate::plan::validate_path(&file.path)?;
    verify(&path.join(&file.path), file)?;
  }
  installed.directory = path.into();
  installed.entrypoint = if installed.plan.entrypoint == "." {
    path.into()
  } else {
    crate::plan::validate_path(&installed.plan.entrypoint)?;
    path.join(&installed.plan.entrypoint)
  };
  installed.state = "installed_unverified".into();
  Ok(installed)
}

pub(crate) fn digest(path: &Path) -> Result<String> {
  let mut digest = Sha256::new();
  let mut file = File::open(path)?;
  let mut buffer = [0u8; 65536];
  loop {
    let count = file.read(&mut buffer)?;
    if count == 0 {
      break;
    }
    digest.update(&buffer[..count]);
  }
  Ok(hex::encode(digest.finalize()))
}

pub(crate) fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
  let parent = path
    .parent()
    .ok_or_else(|| anyhow::anyhow!("manifest parent missing"))?;
  let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
  serde_json::to_writer(&mut temporary, value)?;
  temporary.flush()?;
  temporary.as_file().sync_all()?;
  temporary.persist(path)?;
  Ok(())
}

pub(crate) fn publish(sender: &Option<mpsc::Sender<InstallProgress>>, progress: &InstallProgress) {
  if let Some(sender) = sender {
    let _ = sender.try_send(progress.clone());
  }
}

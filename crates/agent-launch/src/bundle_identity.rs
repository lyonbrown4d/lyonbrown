//! Content identity for retained Unix app bundles, without following tree links.
use std::{
  fs,
  io::Read,
  os::unix::{ffi::OsStrExt, fs::PermissionsExt},
  path::Path,
};

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Hash the complete tree, including Unix modes and relative symlink targets.
/// Links must resolve within this bundle; special files and linked roots fail.
pub fn bundle_identity(root: &Path) -> Result<String> {
  let metadata = fs::symlink_metadata(root).context("inspect native app bundle root")?;
  ensure!(
    metadata.is_dir(),
    "native app bundle root must be a real directory"
  );
  let canonical_root = root.canonicalize()?;
  let mut entries = WalkDir::new(root)
    .follow_links(false)
    .into_iter()
    .collect::<std::result::Result<Vec<_>, _>>()?;
  entries.sort_by(|left, right| {
    left
      .path()
      .as_os_str()
      .as_bytes()
      .cmp(right.path().as_os_str().as_bytes())
  });
  let mut digest = Sha256::new();
  digest.update(b"lyonbrown-app-bundle-v1\0");
  for entry in entries {
    let path = entry.path();
    let relative = path.strip_prefix(root)?;
    frame(&mut digest, relative.as_os_str().as_bytes());
    let metadata = fs::symlink_metadata(path)?;
    let kind = metadata.file_type();
    digest.update((metadata.permissions().mode() & 0o7777).to_le_bytes());
    if kind.is_symlink() {
      digest.update(b"L");
      let target = fs::read_link(path)?;
      ensure!(
        !target.is_absolute(),
        "absolute symlink in native app bundle: {}",
        relative.display()
      );
      let resolved = path
        .canonicalize()
        .with_context(|| format!("resolve bundle symlink {}", relative.display()))?;
      ensure!(
        resolved.starts_with(&canonical_root),
        "symlink escapes native app bundle: {}",
        relative.display()
      );
      frame(&mut digest, target.as_os_str().as_bytes());
    } else if kind.is_dir() {
      digest.update(b"D");
    } else if kind.is_file() {
      digest.update(b"F");
      digest.update(metadata.len().to_le_bytes());
      let mut file = fs::File::open(path)?;
      let mut total = 0_u64;
      let mut buffer = [0_u8; 65_536];
      loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
          break;
        }
        total += size as u64;
        digest.update(&buffer[..size]);
      }
      ensure!(
        total == metadata.len(),
        "bundle file changed while hashing: {}",
        relative.display()
      );
    } else {
      anyhow::bail!(
        "unsupported special file in native app bundle: {}",
        relative.display()
      );
    }
  }
  Ok(hex::encode(digest.finalize()))
}

fn frame(digest: &mut Sha256, bytes: &[u8]) {
  digest.update((bytes.len() as u64).to_le_bytes());
  digest.update(bytes);
}

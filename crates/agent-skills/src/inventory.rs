use std::{
  collections::BTreeSet,
  fs,
  io::{Read, Write},
  path::Path,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Limits, ParsedSkill, Result, SkillError, SkillManifest, parse_manifest, paths};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileEntry {
  pub path: String,
  pub bytes: u64,
  pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillInspection {
  pub manifest: SkillManifest,
  pub files: Vec<FileEntry>,
  pub total_bytes: u64,
  /// Hash of ordered path, size and file-content hashes. Independent of timestamps.
  pub content_sha256: String,
}

pub(crate) fn inspect(root: &Path, limits: Limits) -> Result<SkillInspection> {
  let mut files = Vec::new();
  let mut seen = BTreeSet::new();
  let mut bytes = 0;
  walk(root, root, limits, &mut seen, &mut files, &mut bytes)?;
  files.sort_by(|left, right| left.path.cmp(&right.path));
  let content = fs::read_to_string(root.join("SKILL.md"))
    .map_err(|_| SkillError::Manifest("missing or non-UTF-8 SKILL.md".into()))?;
  let ParsedSkill { manifest, .. } = parse_manifest(&content)?;
  if root.file_name().and_then(|name| name.to_str()) != Some(manifest.name.as_str()) {
    return Err(SkillError::Manifest(
      "directory name differs from Skill name".into(),
    ));
  }
  let mut digest = Sha256::new();
  for file in &files {
    digest.update((file.path.len() as u64).to_le_bytes());
    digest.update(file.path.as_bytes());
    digest.update(file.bytes.to_le_bytes());
    digest.update(file.sha256.as_bytes());
  }
  Ok(SkillInspection {
    manifest,
    files,
    total_bytes: bytes,
    content_sha256: hex::encode(digest.finalize()),
  })
}

fn walk(
  root: &Path,
  current: &Path,
  limits: Limits,
  seen: &mut BTreeSet<String>,
  files: &mut Vec<FileEntry>,
  bytes: &mut u64,
) -> Result<()> {
  if !fs::symlink_metadata(current)?.is_dir() {
    return Err(SkillError::UnsupportedEntry);
  }
  for entry in fs::read_dir(current)? {
    let entry = entry?;
    let path = entry.path();
    let relative = path
      .strip_prefix(root)
      .map_err(|_| SkillError::UnsafePath)?;
    let raw = relative
      .to_str()
      .ok_or(SkillError::UnsafePath)?
      .replace(std::path::MAIN_SEPARATOR, "/");
    paths::relative(&raw, limits.depth)?;
    paths::unique(seen, &raw)?;
    if seen.len() > limits.files {
      return Err(SkillError::Limit("entry count"));
    }
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.is_dir() {
      walk(root, &path, limits, seen, files, bytes)?;
    } else if metadata.is_file() {
      let (size, sha256) = copy_bounded(fs::File::open(&path)?, std::io::sink(), limits, bytes)?;
      if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
      {
        crate::references::validate(&fs::read_to_string(&path)?, &raw, limits.depth)?;
      }
      files.push(FileEntry {
        path: raw,
        bytes: size,
        sha256,
      });
    } else {
      return Err(SkillError::UnsupportedEntry);
    }
  }
  Ok(())
}

pub(crate) fn copy_bounded(
  mut reader: impl Read,
  mut writer: impl Write,
  limits: Limits,
  total: &mut u64,
) -> Result<(u64, String)> {
  let mut digest = Sha256::new();
  let mut size = 0u64;
  let mut buffer = [0u8; 8192];
  loop {
    let count = reader.read(&mut buffer)?;
    if count == 0 {
      break;
    }
    size = size
      .checked_add(count as u64)
      .ok_or(SkillError::Limit("file bytes"))?;
    *total = total
      .checked_add(count as u64)
      .ok_or(SkillError::Limit("extracted bytes"))?;
    if size > limits.file_bytes || *total > limits.extracted_bytes {
      return Err(SkillError::Limit("extracted or file bytes"));
    }
    digest.update(&buffer[..count]);
    writer.write_all(&buffer[..count])?;
  }
  Ok((size, hex::encode(digest.finalize())))
}

pub(crate) fn copy_directory(source: &Path, destination: &Path, limits: Limits) -> Result<()> {
  // Inspect first to reject symlinks, special files, and non-portable paths.
  let inspection = inspect(source, limits)?;
  fs::create_dir(destination)?;
  let mut bytes = 0;
  for file in inspection.files {
    let path = paths::relative(&file.path, limits.depth)?;
    let target = destination.join(&path);
    if let Some(parent) = target.parent() {
      fs::create_dir_all(parent)?;
    }
    let metadata = fs::symlink_metadata(source.join(&path))?;
    if !metadata.is_file() {
      return Err(SkillError::UnsupportedEntry);
    }
    let mut output = fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(target)?;
    let (size, hash) = copy_bounded(
      fs::File::open(source.join(path))?,
      &mut output,
      limits,
      &mut bytes,
    )?;
    output.sync_all()?;
    if size != file.bytes || hash != file.sha256 {
      return Err(SkillError::ContentChanged);
    }
  }
  Ok(())
}

use std::{
  collections::BTreeSet,
  fs,
  io::{Read, Seek, SeekFrom},
  path::Path,
};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use crate::{Limits, Result, SkillError, inventory::copy_bounded, paths};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormat {
  Zip,
  Tar,
  TarGz,
}

pub(crate) fn extract(
  mut reader: impl Read + Seek,
  format: ArchiveFormat,
  destination: &Path,
  limits: Limits,
) -> Result<()> {
  if reader.seek(SeekFrom::End(0))? > limits.download_bytes {
    return Err(SkillError::Limit("download bytes"));
  }
  reader.seek(SeekFrom::Start(0))?;
  match format {
    ArchiveFormat::Zip => zip(reader, destination, limits),
    ArchiveFormat::Tar => tar(reader, destination, limits),
    ArchiveFormat::TarGz => tar(GzDecoder::new(reader), destination, limits),
  }
}

fn zip(reader: impl Read + Seek, destination: &Path, limits: Limits) -> Result<()> {
  let mut archive = zip::ZipArchive::new(reader).map_err(zip_error)?;
  if archive.len() > limits.files {
    return Err(SkillError::Limit("entry count"));
  }
  let mut seen = BTreeSet::new();
  let mut bytes = 0;
  for index in 0..archive.len() {
    let mut entry = archive.by_index(index).map_err(zip_error)?;
    let directory = entry.is_dir();
    let name = entry.name().map_err(zip_error)?;
    let raw = name.trim_end_matches('/');
    let target = destination.join(paths::relative(raw, limits.depth)?);
    paths::unique(&mut seen, raw)?;
    if let Some(mode) = entry.unix_mode() {
      let kind = mode & 0o170000;
      if kind != 0 && kind != if directory { 0o040000 } else { 0o100000 } {
        return Err(SkillError::UnsupportedEntry);
      }
    }
    if directory {
      fs::create_dir_all(target)?;
    } else {
      if entry.size() > limits.file_bytes {
        return Err(SkillError::Limit("file bytes"));
      }
      if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
      }
      let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
      copy_bounded(&mut entry, &mut output, limits, &mut bytes)?;
      output.sync_all()?;
    }
  }
  Ok(())
}

fn tar(reader: impl Read, destination: &Path, limits: Limits) -> Result<()> {
  let mut archive = tar::Archive::new(reader);
  let mut seen = BTreeSet::new();
  let mut bytes = 0;
  // Raw iteration prevents unbounded extension records being buffered by tar.
  // GNU/PAX/sparse extensions are deliberately rejected as unsupported entries.
  for entry in archive.entries()?.raw(true) {
    let mut entry = entry?;
    let kind = entry.header().entry_type();
    if !kind.is_file() && !kind.is_dir() {
      return Err(SkillError::UnsupportedEntry);
    }
    let raw = entry.path_bytes();
    let raw = std::str::from_utf8(&raw).map_err(|_| SkillError::UnsafePath)?;
    let raw = raw.trim_end_matches('/');
    let target = destination.join(paths::relative(raw, limits.depth)?);
    paths::unique(&mut seen, raw)?;
    if seen.len() > limits.files {
      return Err(SkillError::Limit("entry count"));
    }
    if kind.is_dir() {
      if entry.size() != 0 {
        return Err(SkillError::Archive("directory contains file data".into()));
      }
      fs::create_dir_all(target)?;
    } else {
      if entry.size() > limits.file_bytes {
        return Err(SkillError::Limit("file bytes"));
      }
      if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
      }
      let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
      copy_bounded(&mut entry, &mut output, limits, &mut bytes)?;
      output.sync_all()?;
    }
  }
  Ok(())
}

fn zip_error(error: zip::result::ZipError) -> SkillError {
  SkillError::Archive(error.to_string())
}

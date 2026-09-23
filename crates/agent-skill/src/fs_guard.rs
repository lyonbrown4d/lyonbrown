use super::{Dir, Path, SkillError, fs};

#[cfg(unix)]
pub(super) fn open_skill_dir(root: &Path, before: &std::fs::Metadata) -> Result<Dir, SkillError> {
  use cap_std::fs::MetadataExt as CapMetadataExt;
  use std::os::unix::fs::MetadataExt as StdMetadataExt;

  let directory =
    Dir::open_ambient_dir(root, cap_std::ambient_authority()).map_err(|source| SkillError::Io {
      path: root.to_path_buf(),
      source,
    })?;
  let opened = directory.dir_metadata().map_err(|source| SkillError::Io {
    path: root.to_path_buf(),
    source,
  })?;
  if StdMetadataExt::dev(before) != CapMetadataExt::dev(&opened)
    || StdMetadataExt::ino(before) != CapMetadataExt::ino(&opened)
  {
    return Err(SkillError::Linked(root.to_path_buf()));
  }
  Ok(directory)
}

#[cfg(windows)]
pub(super) fn open_skill_dir(root: &Path, _before: &std::fs::Metadata) -> Result<Dir, SkillError> {
  use std::os::windows::fs::OpenOptionsExt;

  const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
  const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
  let file = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
    .open(root)
    .map_err(|source| SkillError::Io {
      path: root.to_path_buf(),
      source,
    })?;
  let metadata = file.metadata().map_err(|source| SkillError::Io {
    path: root.to_path_buf(),
    source,
  })?;
  if is_link(&metadata) {
    return Err(SkillError::Linked(root.to_path_buf()));
  }
  Ok(Dir::from_std_file(file))
}

#[cfg(not(any(unix, windows)))]
pub(super) fn open_skill_dir(root: &Path, _before: &std::fs::Metadata) -> Result<Dir, SkillError> {
  Dir::open_ambient_dir(root, cap_std::ambient_authority()).map_err(|source| SkillError::Io {
    path: root.to_path_buf(),
    source,
  })
}

pub(super) fn validate_unlinked_ancestors(path: &Path) -> Result<(), SkillError> {
  for ancestor in path.ancestors() {
    match fs::symlink_metadata(ancestor) {
      Ok(metadata) if is_link(&metadata) => {
        return Err(SkillError::Linked(ancestor.to_path_buf()));
      }
      Ok(_) => {}
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
      Err(source) => {
        return Err(SkillError::Io {
          path: ancestor.to_path_buf(),
          source,
        });
      }
    }
  }
  Ok(())
}

#[cfg(not(windows))]
pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
  metadata.file_type().is_symlink()
}

#[cfg(not(windows))]
pub(super) fn is_cap_link(metadata: &cap_std::fs::Metadata) -> bool {
  metadata.file_type().is_symlink()
}

#[cfg(windows)]
pub(super) fn is_cap_link(metadata: &cap_std::fs::Metadata) -> bool {
  use cap_std::fs::MetadataExt;
  const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
  metadata.file_type().is_symlink()
    || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(windows)]
pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
  use std::os::windows::fs::MetadataExt;
  const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
  metadata.file_type().is_symlink()
    || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

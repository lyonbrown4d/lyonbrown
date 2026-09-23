use std::{
  io::Read,
  path::{Path, PathBuf},
};

use cap_std::{ambient_authority, fs::Dir};
use ignore::{DirEntry, WalkBuilder};

use crate::IndexError;

const MAX_INDEX_FILE_BYTES: u64 = 512 * 1024;
const INDEXED_EXTENSIONS: &[&str] = &[
  "c", "cc", "cpp", "cs", "css", "go", "h", "hpp", "html", "java", "js", "jsx", "json", "kt", "md",
  "mdx", "py", "rb", "rs", "sh", "toml", "ts", "tsx", "txt", "xml", "yaml", "yml",
];

pub(crate) fn workspace_files(root: &Path) -> Result<Vec<(PathBuf, String)>, IndexError> {
  let directory = Dir::open_ambient_dir(root, ambient_authority())?;
  let mut files = Vec::new();
  for entry in walker(root) {
    let entry = match entry {
      Ok(entry) => entry,
      Err(error) => {
        tracing::warn!(%error, "workspace walk skipped an inaccessible path");
        continue;
      }
    };
    let Some(relative) = relative_file(root, &entry) else {
      continue;
    };
    if !relative.to_str().is_some() {
      continue;
    }
    match read_indexable_from_dir(&directory, &relative) {
      Ok(Some(content)) => files.push((relative, content)),
      Ok(None) => {}
      Err(error) => {
        tracing::warn!(path = %relative.display(), %error, "workspace index skipped an unreadable file")
      }
    }
  }
  Ok(files)
}

pub(crate) fn read_indexable(root: &Path, relative: &Path) -> Result<Option<String>, IndexError> {
  if !eligible_path(relative) {
    return Ok(None);
  }
  let directory = Dir::open_ambient_dir(root, ambient_authority())?;
  read_indexable_from_dir(&directory, relative)
}

fn read_indexable_from_dir(directory: &Dir, relative: &Path) -> Result<Option<String>, IndexError> {
  if !eligible_path(relative) {
    return Ok(None);
  }
  let metadata = match directory.symlink_metadata(relative) {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(IndexError::Io(error)),
  };
  if metadata.file_type().is_symlink()
    || !metadata.is_file()
    || metadata.len() > MAX_INDEX_FILE_BYTES
  {
    return Ok(None);
  }
  let mut file = match directory.open(relative) {
    Ok(file) => file,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(IndexError::Io(error)),
  };
  let mut bytes = Vec::with_capacity(metadata.len() as usize);
  file
    .by_ref()
    .take(MAX_INDEX_FILE_BYTES + 1)
    .read_to_end(&mut bytes)?;
  if bytes.len() as u64 > MAX_INDEX_FILE_BYTES {
    return Ok(None);
  }
  Ok(String::from_utf8(bytes).ok())
}

fn walker(root: &Path) -> impl Iterator<Item = Result<DirEntry, ignore::Error>> {
  WalkBuilder::new(root)
    .hidden(false)
    .follow_links(false)
    .require_git(false)
    .filter_entry(|entry| !excluded_directory(entry))
    .build()
}

fn relative_file(root: &Path, entry: &DirEntry) -> Option<PathBuf> {
  if !entry.file_type().is_some_and(|kind| kind.is_file()) {
    return None;
  }
  entry.path().strip_prefix(root).ok().map(Path::to_path_buf)
}

fn eligible_path(path: &Path) -> bool {
  if path.components().any(|component| {
    let part = component.as_os_str().to_string_lossy().to_ascii_lowercase();
    is_excluded_directory(&part)
  }) {
    return false;
  }
  let name = path
    .file_name()
    .unwrap_or_default()
    .to_string_lossy()
    .to_ascii_lowercase();
  if name.starts_with(".env")
    || name.contains("secret")
    || name.contains("credential")
    || matches!(name.as_str(), ".npmrc" | ".pypirc")
    || path
      .extension()
      .and_then(|extension| extension.to_str())
      .is_some_and(|extension| {
        matches!(
          extension.to_ascii_lowercase().as_str(),
          "pem" | "p12" | "pfx" | "key"
        )
      })
  {
    return false;
  }
  path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| INDEXED_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
}

fn excluded_directory(entry: &DirEntry) -> bool {
  if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
    return false;
  }
  let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
  is_excluded_directory(&name)
}

fn is_excluded_directory(name: &str) -> bool {
  matches!(
    name,
    ".agent"
      | ".aws"
      | ".git"
      | ".gnupg"
      | ".ssh"
      | ".venv"
      | "build"
      | "coverage"
      | "dist"
      | "node_modules"
      | "out"
      | "target"
      | "venv"
  )
}

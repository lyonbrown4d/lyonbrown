//! Zero-configuration workspace detection and canonical repository instructions.

use std::{
  io::Read,
  path::{Path, PathBuf},
};

use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod identity;

pub use identity::workspace_identity;

const INSTRUCTION_FILES: [&str; 4] = ["AGENTS.md", "CLAUDE.md", "README.md", "CONTRIBUTING.md"];

/// Maximum accepted size of one repository instruction file (256 KiB).
pub const MAX_INSTRUCTION_FILE_BYTES: usize = 256 * 1024;
/// Maximum accepted total size of canonical repository instructions (512 KiB).
pub const MAX_AGGREGATE_INSTRUCTION_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectType {
  Rust,
  Go,
  Maven,
  Gradle,
  Node,
  #[default]
  Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryInstruction {
  pub path: String,
  pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProfile {
  pub root: PathBuf,
  pub project_type: ProjectType,
  pub languages: Vec<String>,
  pub frameworks: Vec<String>,
  pub build_commands: Vec<String>,
  pub test_commands: Vec<String>,
  pub conventions: Vec<String>,
  pub enabled_skills: Vec<String>,
  pub repository_instructions: Vec<RepositoryInstruction>,
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
  #[error("workspace path {path} is not a directory")]
  NotDirectory { path: PathBuf },
  #[error("could not read {path}: {source}")]
  Io {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("repository instruction {path} exceeds {limit} bytes (observed at least {bytes})")]
  InstructionTooLarge {
    path: PathBuf,
    bytes: u64,
    limit: usize,
  },
  #[error("repository instruction aggregate exceeds {limit} bytes")]
  AggregateTooLarge { limit: usize },
  #[error("repository instruction {path} is not valid UTF-8")]
  InvalidUtf8 { path: PathBuf },
  #[error("workspace root {path} must not be a symbolic link or junction")]
  LinkedRoot { path: PathBuf },
  #[error("repository instruction {path} must not be a symbolic link or junction")]
  LinkedInstruction { path: PathBuf },
}

impl WorkspaceProfile {
  pub fn detect(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
    let root = root.as_ref();
    if !root.is_dir() {
      return Err(WorkspaceError::NotDirectory {
        path: root.to_path_buf(),
      });
    }
    let (project_type, languages, build_commands, test_commands) = detect_manifest(root);
    Ok(Self {
      root: root.to_path_buf(),
      project_type,
      languages,
      frameworks: Vec::new(),
      build_commands,
      test_commands,
      conventions: Vec::new(),
      enabled_skills: Vec::new(),
      repository_instructions: load_repository_instructions(root)?,
    })
  }
}

pub fn load_repository_instructions(
  root: impl AsRef<Path>,
) -> Result<Vec<RepositoryInstruction>, WorkspaceError> {
  let root = root.as_ref();
  let root_metadata = std::fs::symlink_metadata(root).map_err(|source| WorkspaceError::Io {
    path: root.to_path_buf(),
    source,
  })?;
  if root_metadata.file_type().is_symlink() {
    return Err(WorkspaceError::LinkedRoot {
      path: root.to_path_buf(),
    });
  }
  let directory = open_workspace_dir(root, &root_metadata)?;
  let mut instructions = Vec::new();
  let mut aggregate = 0_usize;
  for name in INSTRUCTION_FILES {
    let path = root.join(name);
    let metadata = match directory.symlink_metadata(name) {
      Ok(metadata) => metadata,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
      Err(source) => return Err(WorkspaceError::Io { path, source }),
    };
    if metadata.file_type().is_symlink() {
      return Err(WorkspaceError::LinkedInstruction { path });
    }
    if metadata.len() > MAX_INSTRUCTION_FILE_BYTES as u64 {
      return Err(WorkspaceError::InstructionTooLarge {
        path,
        bytes: metadata.len(),
        limit: MAX_INSTRUCTION_FILE_BYTES,
      });
    }
    let mut file = directory.open(name).map_err(|source| WorkspaceError::Io {
      path: path.clone(),
      source,
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    (&mut file)
      .take((MAX_INSTRUCTION_FILE_BYTES + 1) as u64)
      .read_to_end(&mut bytes)
      .map_err(|source| WorkspaceError::Io {
        path: path.clone(),
        source,
      })?;
    if bytes.len() > MAX_INSTRUCTION_FILE_BYTES {
      return Err(WorkspaceError::InstructionTooLarge {
        path,
        bytes: bytes.len() as u64,
        limit: MAX_INSTRUCTION_FILE_BYTES,
      });
    }
    aggregate = aggregate
      .checked_add(bytes.len())
      .ok_or(WorkspaceError::AggregateTooLarge {
        limit: MAX_AGGREGATE_INSTRUCTION_BYTES,
      })?;
    if aggregate > MAX_AGGREGATE_INSTRUCTION_BYTES {
      return Err(WorkspaceError::AggregateTooLarge {
        limit: MAX_AGGREGATE_INSTRUCTION_BYTES,
      });
    }
    let content =
      String::from_utf8(bytes).map_err(|_| WorkspaceError::InvalidUtf8 { path: path.clone() })?;
    instructions.push(RepositoryInstruction {
      path: name.into(),
      content,
    });
  }
  Ok(instructions)
}

#[cfg(unix)]
fn open_workspace_dir(root: &Path, before: &std::fs::Metadata) -> Result<Dir, WorkspaceError> {
  use cap_std::fs::MetadataExt as CapMetadataExt;
  use std::os::unix::fs::MetadataExt as StdMetadataExt;

  let directory = Dir::open_ambient_dir(root, cap_std::ambient_authority()).map_err(|source| {
    WorkspaceError::Io {
      path: root.to_path_buf(),
      source,
    }
  })?;
  let opened = directory
    .dir_metadata()
    .map_err(|source| WorkspaceError::Io {
      path: root.to_path_buf(),
      source,
    })?;
  if StdMetadataExt::dev(before) != CapMetadataExt::dev(&opened)
    || StdMetadataExt::ino(before) != CapMetadataExt::ino(&opened)
  {
    return Err(WorkspaceError::LinkedRoot {
      path: root.to_path_buf(),
    });
  }
  Ok(directory)
}

#[cfg(windows)]
fn open_workspace_dir(root: &Path, _before: &std::fs::Metadata) -> Result<Dir, WorkspaceError> {
  use std::os::windows::fs::OpenOptionsExt;

  const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
  const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
  let file = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
    .open(root)
    .map_err(|source| WorkspaceError::Io {
      path: root.to_path_buf(),
      source,
    })?;
  let metadata = file.metadata().map_err(|source| WorkspaceError::Io {
    path: root.to_path_buf(),
    source,
  })?;
  if metadata.file_type().is_symlink() {
    return Err(WorkspaceError::LinkedRoot {
      path: root.to_path_buf(),
    });
  }
  Ok(Dir::from_std_file(file))
}

#[cfg(not(any(unix, windows)))]
fn open_workspace_dir(root: &Path, _before: &std::fs::Metadata) -> Result<Dir, WorkspaceError> {
  Dir::open_ambient_dir(root, cap_std::ambient_authority()).map_err(|source| WorkspaceError::Io {
    path: root.to_path_buf(),
    source,
  })
}

pub fn workspace_cache_path(
  home_root: impl AsRef<Path>,
  workspace_root: impl AsRef<Path>,
) -> Result<PathBuf, WorkspaceError> {
  let workspace_root = workspace_root.as_ref();
  let identity = workspace_identity(workspace_root)?;
  let mut hash = 0xcbf29ce484222325_u64;
  for byte in &identity {
    hash ^= u64::from(*byte);
    hash = hash.wrapping_mul(0x100000001b3);
  }
  Ok(
    home_root
      .as_ref()
      .join(".agent")
      .join("workspaces")
      .join(format!("{hash:016x}")),
  )
}

fn detect_manifest(root: &Path) -> (ProjectType, Vec<String>, Vec<String>, Vec<String>) {
  if root.join("Cargo.toml").is_file() {
    return (
      ProjectType::Rust,
      vec!["Rust".into()],
      vec!["cargo build".into()],
      vec!["cargo test".into()],
    );
  }
  if root.join("go.mod").is_file() {
    return (
      ProjectType::Go,
      vec!["Go".into()],
      vec!["go build ./...".into()],
      vec!["go test ./...".into()],
    );
  }
  if root.join("pom.xml").is_file() {
    return (
      ProjectType::Maven,
      vec!["Java".into()],
      vec!["mvn package".into()],
      vec!["mvn test".into()],
    );
  }
  if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
    return (
      ProjectType::Gradle,
      vec!["Java".into()],
      vec!["./gradlew build".into()],
      vec!["./gradlew test".into()],
    );
  }
  if root.join("package.json").is_file() {
    return (
      ProjectType::Node,
      vec!["JavaScript".into()],
      vec!["npm run build".into()],
      vec!["npm test".into()],
    );
  }
  (ProjectType::Unknown, Vec::new(), Vec::new(), Vec::new())
}

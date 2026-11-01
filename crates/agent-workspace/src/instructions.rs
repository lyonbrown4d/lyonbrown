//! Bounded loading of repository-provided agent instructions.
//!
//! These instructions are untrusted repository content. Callers must keep them
//! below system and application policy, and must never use them to grant tools
//! or widen the authorized workspace.

use crate::Workspace;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

const INSTRUCTIONS_FILE: &str = "AGENTS.md";
const MAX_INSTRUCTION_BYTES: usize = 32 * 1024;
const MAX_TOTAL_BYTES: usize = 64 * 1024;
const MAX_NESTED_DIRECTORIES: usize = 32;
const MAX_CONTEXT_PATHS: usize = 256;
const MAX_CONTEXT_DIRECTORIES: usize = 1_600;

/// One repository instruction file and the directory scope it applies to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryInstruction {
  pub relative_path: String,
  pub content: String,
}

/// Loads the root `AGENTS.md`, if present.
///
/// The workspace capability is used for all reads. Missing root instructions
/// produce an empty list; unreadable, invalid UTF-8, or oversized files fail.
pub fn load_repository_instructions(workspace: &Workspace) -> Result<Vec<RepositoryInstruction>> {
  load_instructions(workspace, std::iter::empty())
}

/// Loads root instructions and nested instructions relevant to accessed paths.
///
/// Paths are relative to the authorized workspace. Existing directory
/// ancestors are considered; a missing path stops traversal while preserving
/// instructions already found for its existing ancestors.
pub fn load_repository_instructions_for_paths(
  workspace: &Workspace,
  paths: &[PathBuf],
) -> Result<Vec<RepositoryInstruction>> {
  anyhow::ensure!(
    paths.len() <= MAX_CONTEXT_PATHS,
    "repository instruction context exceeds the {MAX_CONTEXT_PATHS}-path limit"
  );
  let mut directories = BTreeSet::new();
  for path in paths {
    for directory in existing_directory_ancestors(workspace, path)? {
      directories.insert(directory);
      anyhow::ensure!(
        directories.len() <= MAX_CONTEXT_DIRECTORIES,
        "repository instruction context exceeds the {MAX_CONTEXT_DIRECTORIES}-directory limit"
      );
    }
  }
  load_instructions(workspace, directories.into_iter())
}

fn load_instructions(
  workspace: &Workspace,
  nested_directories: impl Iterator<Item = PathBuf>,
) -> Result<Vec<RepositoryInstruction>> {
  let mut directories = vec![PathBuf::new()];
  directories.extend(nested_directories);
  directories.sort_by(|left, right| {
    left
      .components()
      .count()
      .cmp(&right.components().count())
      .then_with(|| left.cmp(right))
  });
  directories.dedup();

  let mut total_bytes = 0usize;
  let mut instructions = Vec::new();
  for directory in directories {
    let relative_path = directory.join(INSTRUCTIONS_FILE);
    let display_path = display_relative_path(&relative_path);
    let metadata = match workspace.dir.symlink_metadata(&relative_path) {
      Ok(metadata) => metadata,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
      Err(error) => {
        return Err(error)
          .with_context(|| format!("inspect repository instructions `{display_path}`"));
      }
    };
    anyhow::ensure!(
      !metadata.file_type().is_symlink(),
      "repository instructions `{display_path}` are a symlink; symlinked instructions are unsupported"
    );
    anyhow::ensure!(
      metadata.is_file(),
      "repository instruction path `{display_path}` is not a regular file"
    );
    anyhow::ensure!(
      metadata.len() <= MAX_INSTRUCTION_BYTES as u64,
      "repository instructions `{display_path}` exceed the {MAX_INSTRUCTION_BYTES}-byte limit"
    );
    let size_total = total_bytes
      .checked_add(metadata.len() as usize)
      .context("repository instruction byte count overflow")?;
    anyhow::ensure!(
      size_total <= MAX_TOTAL_BYTES,
      "repository instructions exceed the {MAX_TOTAL_BYTES}-byte aggregate limit"
    );
    let content = workspace
      .read(&display_path)
      .with_context(|| format!("read repository instructions `{display_path}`"))?;
    anyhow::ensure!(
      content.len() <= MAX_INSTRUCTION_BYTES,
      "repository instructions `{display_path}` exceed the {MAX_INSTRUCTION_BYTES}-byte limit"
    );
    total_bytes = total_bytes
      .checked_add(content.len())
      .context("repository instruction byte count overflow")?;
    anyhow::ensure!(
      total_bytes <= MAX_TOTAL_BYTES,
      "repository instructions exceed the {MAX_TOTAL_BYTES}-byte aggregate limit"
    );
    instructions.push(RepositoryInstruction {
      relative_path: display_path,
      content,
    });
  }
  Ok(instructions)
}

fn existing_directory_ancestors(workspace: &Workspace, path: &Path) -> Result<Vec<PathBuf>> {
  let components = relative_components(path)?;
  let mut current = PathBuf::new();
  let mut directories = Vec::new();
  for component in components {
    let next = current.join(component);
    let expected = display_relative_path(&next);
    let metadata = match workspace.dir.symlink_metadata(&next) {
      Ok(metadata) => metadata,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
      Err(error) => {
        return Err(error).with_context(|| format!("inspect repository context path `{expected}`"));
      }
    };
    anyhow::ensure!(
      !metadata.file_type().is_symlink(),
      "repository instruction context path `{expected}` traverses a symlink; symlinked context paths are unsupported"
    );
    if !metadata.is_dir() {
      break;
    }
    current = next;
    directories.push(current.clone());
    anyhow::ensure!(
      directories.len() <= MAX_NESTED_DIRECTORIES,
      "repository instruction path exceeds the {MAX_NESTED_DIRECTORIES}-directory nesting limit"
    );
  }
  Ok(directories)
}

fn relative_components(path: &Path) -> Result<Vec<&std::ffi::OsStr>> {
  let mut components = Vec::new();
  for component in path.components() {
    match component {
      Component::CurDir => {}
      Component::Normal(name) => {
        anyhow::ensure!(
          name.to_str().is_some(),
          "repository instruction context path contains a non-UTF-8 component"
        );
        components.push(name);
      }
      Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
        bail!("repository instruction context path must stay within the authorized workspace")
      }
    }
  }
  Ok(components)
}

fn display_relative_path(path: &Path) -> String {
  if path.as_os_str().is_empty() {
    ".".to_owned()
  } else {
    #[cfg(windows)]
    {
      path.to_string_lossy().replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
      path.to_string_lossy().into_owned()
    }
  }
}

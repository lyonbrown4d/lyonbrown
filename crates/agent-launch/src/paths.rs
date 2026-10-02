use std::io;
use std::path::{Path, PathBuf};

use agent_local::ScopeFingerprint;
use agent_workspace::WorkspaceError;

pub fn canonicalize_optional_path(path: Option<&Path>) -> io::Result<Option<PathBuf>> {
  path.map(Path::canonicalize).transpose()
}

pub fn scoped_run_directory(
  home: &Path,
  workspace: &Path,
  scope: &ScopeFingerprint,
) -> Result<PathBuf, WorkspaceError> {
  Ok(
    agent_workspace::workspace_cache_path(home, workspace)?
      .join("run")
      .join(scope.as_str()),
  )
}

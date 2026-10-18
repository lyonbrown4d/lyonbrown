use super::{CommandRequest, ResolvedCommand};
use anyhow::{Context, Result, ensure};
use std::{
  collections::{BTreeMap, BTreeSet},
  ffi::OsString,
  path::{Component, Path, PathBuf},
};

/// Trusted immutable configuration. Environment values never originate from command arguments.
#[derive(Clone)]
pub struct CommandPolicy {
  pub(super) executables: BTreeSet<PathBuf>,
  pub(super) environment: BTreeMap<OsString, OsString>,
}
impl CommandPolicy {
  pub fn new(
    executables: impl IntoIterator<Item = PathBuf>,
    environment: BTreeMap<OsString, OsString>,
  ) -> Result<Self> {
    let mut canonical = BTreeSet::new();
    for executable in executables {
      ensure!(
        executable.is_absolute(),
        "allowlisted executable must be absolute"
      );
      let path = executable
        .canonicalize()
        .context("resolve allowlisted executable")?;
      ensure!(path.is_file(), "allowlisted executable must be a file");
      canonical.insert(path);
    }
    for (key, value) in &environment {
      let key = key.to_string_lossy();
      ensure!(
        !key.is_empty() && !key.contains(['=', '\0']),
        "invalid environment key"
      );
      ensure!(
        !value.to_string_lossy().contains('\0'),
        "invalid environment value"
      );
    }
    Ok(Self {
      executables: canonical,
      environment,
    })
  }

  pub(super) fn resolve(&self, root: &Path, request: &CommandRequest) -> Result<ResolvedCommand> {
    ensure!(
      !request.action_id.is_empty() && request.action_id.len() <= 128,
      "invalid action identity"
    );
    ensure!(
      (1..=120_000).contains(&request.timeout_ms),
      "timeout must be 1..120000 ms"
    );
    ensure!(
      (1..=1_048_576).contains(&request.output_limit_bytes),
      "output must be 1..1048576 bytes"
    );
    ensure!(request.arguments.len() <= 256, "too many arguments");
    let mut size = 0usize;
    for argument in &request.arguments {
      ensure!(!argument.contains('\0'), "argument contains NUL");
      size = size
        .checked_add(argument.len())
        .context("argument size overflow")?;
    }
    ensure!(size <= 65_536, "arguments exceed 64 KiB");
    ensure!(
      request.executable.is_absolute(),
      "executable must be absolute"
    );
    let executable = request
      .executable
      .canonicalize()
      .context("resolve executable")?;
    #[cfg(windows)]
    ensure!(
      !executable.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd")
      }),
      "batch files require an implicit shell and are unsupported"
    );
    ensure!(
      self.executables.contains(&executable),
      "executable is not allowlisted"
    );
    ensure!(
      !request.cwd.as_os_str().is_empty()
        && request
          .cwd
          .components()
          .all(|part| { matches!(part, Component::Normal(_) | Component::CurDir) }),
      "cwd must be workspace relative"
    );
    let cwd = root
      .join(&request.cwd)
      .canonicalize()
      .context("resolve cwd")?;
    ensure!(
      cwd.starts_with(root) && cwd.is_dir(),
      "cwd escapes workspace or is not directory"
    );
    Ok(ResolvedCommand { executable, cwd })
  }
}

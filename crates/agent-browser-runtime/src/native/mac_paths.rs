//! Filesystem lookup shared by the macOS CEF host and helper processes.
use anyhow::{Context, ensure};
use std::path::{Path, PathBuf};

fn executable_path(executable: &Path) -> anyhow::Result<PathBuf> {
  let resolved = executable.canonicalize().with_context(|| {
    format!(
      "cannot resolve native client executable: {}",
      executable.display()
    )
  })?;
  ensure!(
    resolved.is_file(),
    "native client executable is not a file: {}",
    resolved.display()
  );
  Ok(resolved)
}

pub(super) fn framework_library(
  executable: &Path,
  configured: Option<PathBuf>,
  sdk: Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
  let executable = executable_path(executable)?;
  let parent = executable
    .parent()
    .context("client executable has no directory")?;
  let framework = "Chromium Embedded Framework.framework/Chromium Embedded Framework";
  let candidates = [
    Some(parent.join("../Frameworks").join(framework)),
    Some(parent.join("../../..").join(framework)),
    configured.map(|path| path.join(framework)),
    sdk.map(|path| path.join(framework)),
  ];
  candidates.into_iter().flatten().find(|path| path.is_file())
    .context("CEF framework missing; package the matching framework in Contents/Frameworks or set CEF_PATH")
}

pub(super) fn helper_executable(executable: &Path) -> anyhow::Result<PathBuf> {
  let executable = executable_path(executable)?;
  let name = executable
    .file_name()
    .and_then(|name| name.to_str())
    .context("native client executable has no UTF-8 filename")?;
  let macos = executable
    .parent()
    .context("native client executable has no directory")?;
  let contents = macos
    .parent()
    .context("native client app bundle is missing")?;
  let bundle = contents
    .parent()
    .context("native client app bundle is missing")?;
  ensure!(
    macos.file_name().is_some_and(|name| name == "MacOS")
      && contents.file_name().is_some_and(|name| name == "Contents")
      && bundle
        .extension()
        .is_some_and(|extension| extension == "app"),
    "macOS CEF requires the native client app bundle and embedded helper bundles; run the packaged client executable"
  );
  let helper = format!("{name} Helper");
  let path = bundle
    .join("Contents/Frameworks")
    .join(format!("{helper}.app/Contents/MacOS/{helper}"));
  ensure!(
    path.is_file(),
    "native client CEF helper bundle is missing: {}",
    path.display()
  );
  Ok(path)
}

pub(super) fn sandbox_library(executable: &Path) -> anyhow::Result<PathBuf> {
  // cef-rs Sandbox::new derives this path from current_exe without resolving
  // the executable first. Validate the same location so an unsupported helper
  // alias fails here instead of reaching the upstream loader's panic path.
  let library = executable
    .parent()
    .context("CEF helper has no directory")?
    .join("../../../Chromium Embedded Framework.framework/Libraries/libcef_sandbox.dylib");
  ensure!(
    library.is_file(),
    "CEF sandbox library is missing from the product helper bundle"
  );
  library
    .canonicalize()
    .context("cannot resolve CEF sandbox library")
}

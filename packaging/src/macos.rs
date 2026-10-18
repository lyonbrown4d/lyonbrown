use anyhow::Result;
#[cfg(not(all(target_os = "macos", feature = "cef-bundle")))]
use anyhow::bail;
use std::path::{Path, PathBuf};

pub fn bundle(
  binaries: &Path,
  output: &Path,
  version: &str,
  cef_runtime: Option<&Path>,
) -> Result<PathBuf> {
  product(
    binaries,
    output,
    version,
    cef_runtime,
    Product {
      executable: "agent-desktop",
      display: "Lyonbrown",
      identifier: "org.lyonbrown.desktop",
      bundle_name: "Lyonbrown.app",
    },
  )
}

/// The TUI's public bin entry may symlink to this app's executable. CEF helper
/// bundles are implementation payload of the same native client product.
pub fn cli_bundle(
  binaries: &Path,
  output: &Path,
  version: &str,
  cef_runtime: Option<&Path>,
) -> Result<PathBuf> {
  product(
    binaries,
    output,
    version,
    cef_runtime,
    Product {
      executable: "agent-cli",
      display: "Lyonbrown CLI",
      identifier: "org.lyonbrown.cli",
      bundle_name: "Lyonbrown CLI.app",
    },
  )
}

struct Product {
  executable: &'static str,
  display: &'static str,
  identifier: &'static str,
  bundle_name: &'static str,
}

#[cfg(all(target_os = "macos", feature = "cef-bundle"))]
fn product(
  binaries: &Path,
  output: &Path,
  version: &str,
  cef_runtime: Option<&Path>,
  product: Product,
) -> Result<PathBuf> {
  use crate::copy_executable;
  let Product {
    executable,
    display,
    identifier,
    bundle_name,
  } = product;
  use anyhow::{Context, ensure};
  use cef::build_util::mac::{BundleInfo, bundle};
  let selected = cef::sys::get_cef_dir()
    .context("CEF SDK was not found; select the matching SDK with CEF_PATH")?
    .canonicalize()?;
  ensure!(
    selected
      .join("Chromium Embedded Framework.framework")
      .is_dir(),
    "selected CEF SDK has no macOS framework"
  );
  if let Some(requested) = cef_runtime {
    let requested = requested.canonicalize().context("resolve --cef-runtime")?;
    ensure!(
      selected == requested
        || selected.starts_with(&requested)
        || requested == selected.join("Chromium Embedded Framework.framework"),
      "--cef-runtime does not match the SDK selected by CEF_PATH; rebuild and package with one matching SDK"
    );
  }
  let target = output.join(bundle_name);
  let temporary = output.join(format!("{executable}.app"));
  ensure!(
    !target.exists() && !temporary.exists(),
    "native app bundle output already exists"
  );
  ensure!(
    binaries.join(executable).is_file(),
    "native client executable is missing"
  );
  // Reuse cef-rs's maintained main/Helper/GPU/Renderer/Plugin/Alerts layout.
  // Every helper copies the same executable and dispatches --type before UI startup.
  let app = bundle(
    output,
    binaries,
    executable,
    executable,
    None,
    BundleInfo::new(
      executable,
      identifier,
      display,
      "English",
      semver::Version::parse(version)?,
    ),
  )?;
  copy_executable(&binaries.join("agentd"), &app.join("Contents/MacOS/agentd"))?;
  if app != target {
    std::fs::rename(&app, &target)?;
  }
  Ok(target)
}

#[cfg(not(all(target_os = "macos", feature = "cef-bundle")))]
fn product(
  _binaries: &Path,
  _output: &Path,
  _version: &str,
  _cef_runtime: Option<&Path>,
  _product: Product,
) -> Result<PathBuf> {
  let _ = (
    _product.executable,
    _product.display,
    _product.identifier,
    _product.bundle_name,
  );
  bail!(
    "native macOS bundles require agent-package built with --features cef-bundle and a matching CEF SDK selected by CEF_PATH"
  )
}

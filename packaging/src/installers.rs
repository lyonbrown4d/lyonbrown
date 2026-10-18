use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result, ensure};

use crate::{copy_executable, run};

/// Delegate archive formats to host packaging tools; never synthesize an installer format.
pub fn package(
  root: &Path,
  binaries: &Path,
  output: &Path,
  version: &str,
  cli_only: bool,
  cef_runtime: Option<&Path>,
) -> Result<()> {
  if cfg!(target_os = "macos") {
    macos(binaries, output, version, cli_only)
  } else if cfg!(target_os = "linux") {
    crate::linux::package(root, binaries, output, version, cli_only, cef_runtime)
  } else if cfg!(target_os = "windows") {
    windows(root, binaries, output, version, cli_only)
  } else {
    anyhow::bail!("native installers are unavailable on this platform")
  }
}

fn windows(
  root: &Path,
  binaries: &Path,
  output: &Path,
  version: &str,
  cli_only: bool,
) -> Result<()> {
  crate::windows::package(root, binaries, output, version, cli_only, None)
}

fn macos(binaries: &Path, output: &Path, version: &str, cli_only: bool) -> Result<()> {
  let stage = output.join("pkg-components");
  ensure!(
    !stage.exists(),
    "PKG stage already exists: {}",
    stage.display()
  );
  let daemon = stage.join("daemon/Library/Application Support/Lyonbrown");
  let cli = stage.join("cli/usr/local/bin");
  let desktop = stage.join("desktop/Applications");
  fs::create_dir_all(&daemon)?;
  fs::create_dir_all(&cli)?;
  fs::create_dir_all(&desktop)?;
  copy_executable(&binaries.join("agentd"), &daemon.join("agentd"))?;
  let cli_bundle = output
    .join(format!(
      "lyonbrown-cli-{version}-macos-{}",
      std::env::consts::ARCH
    ))
    .join("Lyonbrown CLI.app");
  ensure!(
    cli_bundle.is_dir(),
    "CLI CEF app must be staged before PKG creation"
  );
  let installed_cli =
    stage.join("cli/Library/Application Support/Lyonbrown/Clients/Lyonbrown CLI.app");
  fs::create_dir_all(
    installed_cli
      .parent()
      .context("CLI app has no parent directory")?,
  )?;
  run(
    Command::new("ditto").arg(&cli_bundle).arg(&installed_cli),
    "stage CLI native CEF app",
  )?;
  #[cfg(target_os = "macos")]
  std::os::unix::fs::symlink(
    "/Library/Application Support/Lyonbrown/Clients/Lyonbrown CLI.app/Contents/MacOS/agent-cli",
    cli.join("agent"),
  )?;
  if !cli_only {
    let app = output.join("Lyonbrown.app");
    ensure!(
      app.is_dir(),
      "Desktop .app must be staged before PKG creation"
    );
    run(
      Command::new("ditto")
        .arg(&app)
        .arg(desktop.join("Lyonbrown.app")),
      "stage Desktop PKG application",
    )?;
  }
  for (directory, identifier) in [
    ("daemon", "org.lyonbrown.agentd"),
    ("cli", "org.lyonbrown.cli"),
    ("desktop", "org.lyonbrown.desktop"),
  ] {
    if cli_only && directory == "desktop" {
      continue;
    }
    run(
      Command::new("pkgbuild")
        .arg("--root")
        .arg(stage.join(directory))
        .args([
          "--identifier",
          identifier,
          "--version",
          version,
          "--install-location",
          "/",
        ])
        .arg(stage.join(format!("{directory}.pkg"))),
      "build Apple PKG component",
    )?;
  }
  for client in ["cli", "desktop"] {
    if cli_only && client == "desktop" {
      continue;
    }
    let distribution = stage.join(format!("{client}.xml"));
    let xml = format!(
      "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<installer-gui-script minSpecVersion=\"2\"><title>Lyonbrown {client}</title><options customize=\"never\" require-scripts=\"false\"/><choices-outline><line choice=\"daemon\"/><line choice=\"client\"/></choices-outline><choice id=\"daemon\" visible=\"false\"><pkg-ref id=\"org.lyonbrown.agentd\"/></choice><choice id=\"client\" visible=\"false\"><pkg-ref id=\"org.lyonbrown.{client}\"/></choice><pkg-ref id=\"org.lyonbrown.agentd\" version=\"{version}\">daemon.pkg</pkg-ref><pkg-ref id=\"org.lyonbrown.{client}\" version=\"{version}\">{client}.pkg</pkg-ref></installer-gui-script>\n"
    );
    fs::write(&distribution, xml)?;
    let installer = output.join(format!("Lyonbrown-{client}-{version}.pkg"));
    ensure!(
      !installer.exists(),
      "PKG already exists: {}",
      installer.display()
    );
    run(
      Command::new("productbuild")
        .arg("--distribution")
        .arg(distribution)
        .arg("--package-path")
        .arg(&stage)
        .arg(installer),
      "build Apple product installer",
    )?;
  }
  Ok(())
}

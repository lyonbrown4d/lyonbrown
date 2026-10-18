//! Host-native CEF payloads; DEB/RPM metadata owns each client's complete appdir.
use std::path::Path;

use anyhow::Result;

pub fn package(
  root: &Path,
  binaries: &Path,
  output: &Path,
  version: &str,
  cli_only: bool,
  cef_runtime: Option<&Path>,
) -> Result<()> {
  #[cfg(all(target_os = "linux", feature = "cef-bundle"))]
  {
    native::package(root, binaries, output, version, cli_only, cef_runtime)
  }
  #[cfg(not(all(target_os = "linux", feature = "cef-bundle")))]
  {
    let _ = (root, binaries, output, version, cli_only, cef_runtime);
    anyhow::bail!(
      "Linux installers require a Linux host, --features cef-bundle and a matching CEF SDK"
    )
  }
}

#[cfg(all(target_os = "linux", feature = "cef-bundle"))]
mod native {
  use std::{fs, path::Path, process::Command};

  use anyhow::{Context, Result, ensure};

  use crate::run;

  pub(super) fn package(
    root: &Path,
    binaries: &Path,
    output: &Path,
    version: &str,
    cli_only: bool,
    cef_runtime: Option<&Path>,
  ) -> Result<()> {
    ensure!(
      binaries.canonicalize()? == root.join("target/release").canonicalize()?,
      "Linux native installer metadata requires --bin-dir target/release"
    );
    let sdk = cef::sys::get_cef_dir()
      .context("select the matching Linux CEF SDK using CEF_PATH")?
      .canonicalize()?;
    if let Some(requested) = cef_runtime {
      ensure!(
        sdk == requested.canonicalize()?,
        "--cef-runtime does not match CEF_PATH"
      );
    }
    for required in [
      "libcef.so",
      "icudtl.dat",
      "resources.pak",
      "chrome-sandbox",
      "locales/en-US.pak",
    ] {
      ensure!(
        sdk.join(required).is_file(),
        "Linux CEF SDK is missing {required}"
      );
    }
    let stage = binaries.join("lyonbrown-linux");
    ensure!(
      !stage.exists(),
      "Linux package stage already exists: {}",
      stage.display()
    );
    fs::create_dir_all(&stage)?;
    for (client, executable) in [("cli", "agent-cli"), ("desktop", "agent-desktop")] {
      if cli_only && client == "desktop" {
        continue;
      }
      let dynamic = Command::new("readelf")
        .arg("--dynamic")
        .arg(binaries.join(executable))
        .output()
        .context("readelf is required to verify the Linux native client loader path")?;
      ensure!(
        dynamic.status.success(),
        "readelf could not inspect {executable}"
      );
      let dynamic = String::from_utf8(dynamic.stdout)?;
      ensure!(
        dynamic
          .lines()
          .any(|line| line.contains("(NEEDED)") && line.contains("[libcef.so]"))
          && dynamic.lines().any(|line| {
            (line.contains("(RUNPATH)") || line.contains("(RPATH)")) && line.contains("[$ORIGIN]")
          }),
        "{executable} must link native CEF with an exact $ORIGIN runtime search path"
      );
      let client_stage = stage.join(client);
      let runtime = client_stage.join("runtime");
      let programs = client_stage.join("programs");
      fs::create_dir_all(&programs)?;
      // The maintained bundler selects SDK top-level files plus the locales tree.
      cef::build_util::linux::bundle(&runtime, binaries, executable)?;
      for program in [executable, "chrome-sandbox"] {
        fs::rename(runtime.join(program), programs.join(program))?;
      }
      fs::write(
        client_stage.join("launcher"),
        format!("#!/bin/sh\nexec /usr/lib/lyonbrown/{client}/{executable} \"$@\"\n"),
      )?;
    }
    let manifests = root.join("packaging/installers");
    for (directory, package) in [
      ("daemon", "lyonbrown-agentd"),
      ("cli", "lyonbrown-cli"),
      ("desktop", "lyonbrown-desktop"),
    ] {
      if cli_only && directory == "desktop" {
        continue;
      }
      run(
        Command::new("cargo")
          .env("CARGO_TARGET_DIR", root.join("target"))
          .arg("deb")
          .arg("--manifest-path")
          .arg(manifests.join(directory).join("Cargo.toml"))
          .args(["--no-build", "--no-strip", "--output"])
          .arg(output),
        "build DEB using cargo-deb",
      )?;
      run(
        Command::new("cargo")
          .env("CARGO_TARGET_DIR", root.join("target"))
          .current_dir(&manifests)
          .args(["generate-rpm", "-p", directory, "-o"])
          .arg(output.join(format!(
            "{package}-{version}-{}.rpm",
            std::env::consts::ARCH
          ))),
        "build RPM using cargo-generate-rpm",
      )?;
    }
    Ok(())
  }
}

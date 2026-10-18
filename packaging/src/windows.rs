//! Native MSI authoring delegates runtime layout to cef-rs and component discovery to WiX.
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
  #[cfg(all(windows, feature = "cef-bundle"))]
  {
    native::package(root, binaries, output, version, cli_only, cef_runtime)
  }
  #[cfg(not(all(windows, feature = "cef-bundle")))]
  {
    let _ = (root, binaries, output, version, cli_only, cef_runtime);
    anyhow::bail!(
      "Windows MSI packaging must run on Windows with --features cef-bundle and the matching sandbox-enabled CEF SDK selected by CEF_PATH"
    )
  }
}

#[cfg(all(windows, feature = "cef-bundle"))]
mod native {
  use std::{fs, path::Path, process::Command};

  use anyhow::{Context, Result, ensure};

  use crate::copy_executable;

  fn run(command: &mut Command, action: &str) -> Result<()> {
    let result = command.output().with_context(|| action.to_owned())?;
    ensure!(
      result.status.success(),
      "{action}: {}\n{}",
      String::from_utf8_lossy(&result.stdout),
      String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
  }

  pub(super) fn package(
    root: &Path,
    binaries: &Path,
    output: &Path,
    version: &str,
    cli_only: bool,
    cef_runtime: Option<&Path>,
  ) -> Result<()> {
    let parsed = semver::Version::parse(version)?;
    ensure!(
      parsed.pre.is_empty()
        && parsed.build.is_empty()
        && parsed.major <= 255
        && parsed.minor <= 255
        && parsed.patch <= 65535,
      "MSI requires a stable version with major/minor <= 255 and patch <= 65535"
    );
    let architecture = match std::env::consts::ARCH {
      "x86_64" => "x64",
      "aarch64" => "arm64",
      architecture => anyhow::bail!("Windows CEF packaging does not support {architecture}"),
    };
    let sdk = cef::sys::get_cef_dir()
      .context("select the matching Windows CEF SDK using CEF_PATH")?
      .canonicalize()?;
    if let Some(requested) = cef_runtime {
      ensure!(
        sdk == requested.canonicalize().context("resolve --cef-runtime")?,
        "--cef-runtime must be the SDK selected by CEF_PATH"
      );
    }
    for required in [
      "bootstrap.exe",
      "libcef.dll",
      "icudtl.dat",
      "resources.pak",
      "locales/en-US.pak",
    ] {
      ensure!(
        sdk.join(required).is_file(),
        "Windows sandbox CEF SDK is missing {required}"
      );
    }
    for client in ["cli", "desktop"] {
      if cli_only && client == "desktop" {
        continue;
      }
      let stage = output.join(format!("windows-{client}-{version}-{architecture}"));
      ensure!(
        !stage.exists(),
        "Windows staging directory already exists: {}",
        stage.display()
      );
      fs::create_dir_all(&stage)?;
      let payload = stage.join("payload");
      // Rust cdylib artifacts use underscores; bootstrap loads a DLL matching its
      // executable basename. Alias inputs before the maintained CEF bundler runs.
      let (target, executable) = if client == "cli" {
        ("agent_cli", "agent")
      } else {
        ("agent_desktop", "agent-desktop")
      };
      let inputs = stage.join("bundle-inputs");
      fs::create_dir(&inputs)?;
      for extension in ["dll", "pdb"] {
        let source = binaries.join(format!("{target}.{extension}"));
        ensure!(
          source.is_file(),
          "sandbox client library is missing: {}; build the client --lib with native-cef",
          source.display()
        );
        fs::copy(&source, inputs.join(format!("{executable}.{extension}")))?;
      }
      cef::build_util::win::bundle(&payload, &inputs, executable)
        .context("bundle native Windows client with sandbox bootstrap and matching CEF runtime")?;
      copy_executable(
        &binaries.join("agent-install.exe"),
        &payload.join("agent-install.exe"),
      )?;
      ensure!(
        binaries.join("agentd.exe").is_file(),
        "agentd.exe is missing"
      );
      let harvest = stage.join("payload.wxs");
      run(
        Command::new("heat.exe")
          .arg("dir")
          .arg(&payload)
          .args([
            "-nologo",
            "-ag",
            "-srd",
            "-sreg",
            "-scom",
            "-sfrag",
            "-dr",
            "CLIENTDIR",
            "-cg",
            "ClientPayload",
            "-var",
            "var.PayloadDir",
            "-out",
          ])
          .arg(&harvest),
        "harvest complete Windows CEF client payload using WiX",
      )?;
      let product_object = stage.join("product.wixobj");
      let payload_object = stage.join("payload.wixobj");
      let daemon_object = stage.join("daemon.wixobj");
      for (source, object) in [
        (
          root.join(format!("packaging/windows/{client}.wxs")),
          &product_object,
        ),
        (harvest, &payload_object),
        (root.join("packaging/windows/daemon.wxs"), &daemon_object),
      ] {
        run(
          Command::new("candle.exe")
            .args(["-nologo", "-arch", architecture])
            .arg(format!("-dVersion={version}"))
            .arg(format!("-dBinDir={}", binaries.display()))
            .arg(format!("-dPayloadDir={}", payload.display()))
            .arg("-out")
            .arg(object)
            .arg(source),
          "compile Windows MSI components using WiX",
        )?;
      }
      let installer = output.join(format!("Lyonbrown-{client}-{version}-{architecture}.msi"));
      ensure!(
        !installer.exists(),
        "MSI already exists: {}",
        installer.display()
      );
      run(
        Command::new("light.exe")
          .args(["-nologo", "-out"])
          .arg(&installer)
          .args([product_object, payload_object, daemon_object]),
        "link Windows MSI and validate MSI component rules using WiX",
      )?;
      println!("{}", installer.display());
    }
    Ok(())
  }
}

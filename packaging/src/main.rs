use std::{
  fs,
  path::{Path, PathBuf},
  process::Command,
};

use anyhow::{Context, Result, bail, ensure};

mod installers;
mod linux;
mod macos;
mod windows;

fn main() -> Result<()> {
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../..")
    .canonicalize()?;
  let mut binaries = root.join("target/release");
  let mut output = root.join("dist");
  let mut dmg = false;
  let mut cli_only = false;
  let mut native_installers = false;
  let mut cef_runtime = None;
  let mut args = std::env::args_os().skip(1);
  while let Some(arg) = args.next() {
    match arg.to_str() {
      Some("--bin-dir") => binaries = args.next().context("--bin-dir requires a path")?.into(),
      Some("--output") => output = args.next().context("--output requires a path")?.into(),
      Some("--cef-runtime") => {
        cef_runtime = Some(PathBuf::from(
          args.next().context("--cef-runtime requires a path")?,
        ))
      }
      Some("--dmg") => dmg = true,
      Some("--cli-only") => cli_only = true,
      Some("--native-installers") => native_installers = true,
      Some("--help") => {
        println!(
          "agent-package [--bin-dir target/release] [--output dist] [--cli-only] [--dmg] [--native-installers] [--cef-runtime PATH]"
        );
        return Ok(());
      }
      _ => bail!("unknown packaging option: {}", arg.to_string_lossy()),
    }
  }
  ensure!(
    cfg!(target_os = "macos") || native_installers,
    "non-macOS distributions require --native-installers to include the complete native client runtime"
  );
  ensure!(
    !dmg || (!cli_only && cfg!(target_os = "macos")),
    "--dmg requires the macOS Desktop bundle"
  );
  ensure!(
    !cfg!(target_os = "macos") || cfg!(feature = "cef-bundle"),
    "macOS packages require --features cef-bundle and the matching SDK selected by CEF_PATH"
  );
  fs::create_dir_all(&output)?;
  let output = output.canonicalize()?;
  let version = env!("CARGO_PKG_VERSION");
  if cfg!(target_os = "windows") {
    return windows::package(
      &root,
      &binaries,
      &output,
      version,
      cli_only,
      cef_runtime.as_deref(),
    );
  }
  if native_installers && !cfg!(target_os = "macos") {
    return installers::package(
      &root,
      &binaries,
      &output,
      version,
      cli_only,
      cef_runtime.as_deref(),
    );
  }
  let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
  let name = format!("lyonbrown-cli-{version}-{platform}");
  let stage = output.join(&name);
  ensure!(
    !stage.exists(),
    "packaging staging directory already exists: {}",
    stage.display()
  );
  fs::create_dir_all(stage.join("bin"))?;
  if cfg!(target_os = "macos") {
    let app = macos::cli_bundle(&binaries, &stage, version, cef_runtime.as_deref())?;
    #[cfg(target_os = "macos")]
    std::os::unix::fs::symlink(
      "../Lyonbrown CLI.app/Contents/MacOS/agent-cli",
      stage.join("bin/agent"),
    )?;
    println!("{}", app.display());
  } else {
    copy_executable(&binaries.join("agent-cli"), &stage.join("bin/agent"))?;
  }
  for (source, destination) in [("agentd", "agentd"), ("agent-install", "agent-install")] {
    copy_executable(&binaries.join(source), &stage.join("bin").join(destination))?;
  }
  fs::write(
    stage.join("INSTALL.txt"),
    "Run ./bin/agent-install to install agent and agentd into ~/.local/bin and register the shared per-user native agentd service. Add ~/.local/bin to PATH. --bin-dir PATH selects another installation directory. CLI and Desktop share the service and data.\n",
  )?;
  let archive = output.join(format!("{name}.tar.gz"));
  ensure!(
    !archive.exists(),
    "archive already exists: {}",
    archive.display()
  );
  run(
    Command::new("tar")
      // AppleDouble entries are host metadata, outside the checked product tree.
      .env("COPYFILE_DISABLE", "1")
      .arg("-czf")
      .arg(&archive)
      .arg("-C")
      .arg(&output)
      .arg(&name),
    "create CLI archive",
  )?;
  println!("{}", archive.display());
  if !cli_only {
    let app = macos::bundle(&binaries, &output, version, cef_runtime.as_deref())?;
    println!("{}", app.display());
    if dmg {
      let image = output.join(format!("Lyonbrown-{version}-{platform}.dmg"));
      ensure!(!image.exists(), "DMG already exists: {}", image.display());
      run(
        Command::new("hdiutil")
          .args(["create", "-volname", "Lyonbrown", "-srcfolder"])
          .arg(&app)
          .args(["-format", "UDZO"])
          .arg(&image),
        "create Desktop DMG",
      )?;
      println!("{}", image.display());
    }
  }
  if native_installers {
    installers::package(
      &root,
      &binaries,
      &output,
      version,
      cli_only,
      cef_runtime.as_deref(),
    )?;
  }
  Ok(())
}

pub(crate) fn copy_executable(source: &Path, destination: &Path) -> Result<()> {
  ensure!(
    source.is_file(),
    "required built executable is missing: {}",
    source.display()
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      fs::metadata(source)?.permissions().mode() & 0o111 != 0,
      "source is not executable: {}",
      source.display()
    );
  }
  fs::copy(source, destination).with_context(|| format!("copy {}", source.display()))?;
  Ok(())
}

pub(crate) fn run(command: &mut Command, action: &str) -> Result<()> {
  let result = command.output().with_context(|| action.to_owned())?;
  ensure!(
    result.status.success(),
    "{action}: {}",
    String::from_utf8_lossy(&result.stderr)
  );
  Ok(())
}

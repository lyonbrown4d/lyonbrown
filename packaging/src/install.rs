#[cfg(unix)]
use std::{fs, path::Path, time::Duration};

#[cfg(unix)]
use agent_launch::{LaunchOptions, ensure_daemon};
use anyhow::Result;
#[cfg(unix)]
use anyhow::{Context, bail, ensure};
#[cfg(unix)]
use directories::BaseDirs;
#[cfg(unix)]
use semver::Version;

#[cfg(target_os = "macos")]
mod install_bundle;
#[cfg(windows)]
mod install_windows;

#[cfg(windows)]
#[tokio::main]
async fn main() -> Result<()> {
  install_windows::register().await
}

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<()> {
  let mut destination = BaseDirs::new()
    .context("locate user home")?
    .home_dir()
    .join(".local/bin");
  let mut args = std::env::args_os().skip(1);
  while let Some(arg) = args.next() {
    match arg.to_str() {
      Some("--bin-dir") => destination = args.next().context("--bin-dir requires a path")?.into(),
      Some("--help") => {
        println!(
          "agent-install [--bin-dir PATH]\nInstalls CLI + agentd and registers the shared per-user native service."
        );
        return Ok(());
      }
      _ => bail!("unknown installation option: {}", arg.to_string_lossy()),
    }
  }
  ensure!(
    cfg!(target_os = "macos") || cfg!(target_os = "linux"),
    "Windows per-user SCM installation is unavailable; no background process fallback is provided"
  );
  let installer = std::env::current_exe()?;
  let source = installer
    .parent()
    .context("installer has no parent directory")?;
  ensure!(
    source.join("agent").is_file(),
    "package is missing the agent executable"
  );
  let bundled = source.join("agentd");
  let version = daemon_version(&bundled).await?;
  if destination.join("agentd").exists() {
    let installed = daemon_version(&destination.join("agentd")).await?;
    ensure!(
      version >= installed,
      "refusing to downgrade installed agentd {installed} to {version}"
    );
  }
  // The shared launcher retains its own stable versioned daemon before registering it.
  let mut options = LaunchOptions::bundled(["sessions", "runs"])?;
  options.bundled_agentd = bundled;
  ensure_daemon(options).await?;
  fs::create_dir_all(&destination)?;
  #[cfg(target_os = "macos")]
  install_bundle::install_native_cli(source, &destination, &version)?;
  let names: &[&str] = if cfg!(target_os = "macos") {
    &["agentd"]
  } else {
    &["agent", "agentd"]
  };
  for name in names {
    let pending = destination.join(format!("{name}.install-pending"));
    fs::copy(source.join(name), &pending)?;
    fs::File::open(&pending)?.sync_all()?;
    fs::rename(pending, destination.join(name))?;
  }
  println!(
    "Installed {} ({}). Add {} to PATH. The shared native agentd service is ready.",
    destination.join("agent").display(),
    version,
    destination.display()
  );
  Ok(())
}

#[cfg(unix)]
async fn daemon_version(executable: &Path) -> Result<Version> {
  let result = tokio::time::timeout(
    Duration::from_secs(3),
    tokio::process::Command::new(executable)
      .arg("--compatibility-json")
      .kill_on_drop(true)
      .output(),
  )
  .await
  .context("daemon version check timed out")??;
  ensure!(
    result.status.success(),
    "daemon compatibility check failed: {}",
    executable.display()
  );
  let value: serde_json::Value = serde_json::from_slice(&result.stdout)?;
  value["daemon_version"]
    .as_str()
    .context("daemon did not report its version")?
    .parse()
    .context("daemon reported an invalid version")
}

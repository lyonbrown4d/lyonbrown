#![cfg(target_os = "macos")]

use anyhow::{Context, Result, ensure};
use plist::{Dictionary, Value};
use std::{
  fs,
  os::unix::fs::{PermissionsExt, symlink},
  path::{Path, PathBuf},
  process::Command,
};
use walkdir::WalkDir;

pub const VERSION: &str = "1.0.0";
pub const DAEMON: &[u8] = b"synthetic shared daemon; never executed";

fn executable(path: &Path, bytes: &[u8]) -> Result<()> {
  fs::create_dir_all(path.parent().context("executable parent")?)?;
  fs::write(path, bytes)?;
  fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
  Ok(())
}

fn bundle(path: &Path, client: &str) -> Result<()> {
  let executable_name = format!("agent-{client}");
  let info = Dictionary::from_iter([
    (
      "CFBundleIdentifier",
      Value::String(format!("org.lyonbrown.{client}")),
    ),
    ("CFBundleExecutable", Value::String(executable_name.clone())),
    ("CFBundleVersion", Value::String(VERSION.into())),
    ("CFBundleShortVersionString", Value::String(VERSION.into())),
    ("CFBundlePackageType", Value::String("APPL".into())),
  ]);
  executable(
    &path.join("Contents/MacOS").join(&executable_name),
    b"unchanged main",
  )?;
  Value::Dictionary(info.clone()).to_file_xml(path.join("Contents/Info.plist"))?;
  let framework = path.join("Contents/Frameworks/CEF.framework");
  fs::create_dir_all(framework.join("Versions/A/Resources"))?;
  Value::Dictionary(Dictionary::from_iter([
    (
      "CFBundleIdentifier",
      Value::String("org.chromium.framework".into()),
    ),
    ("CFBundleVersion", Value::String("154.0.34".into())),
    ("CFBundlePackageType", Value::String("FMWK".into())),
  ]))
  .to_file_xml(framework.join("Versions/A/Resources/Info.plist"))?;
  fs::write(
    framework.join("Versions/A/Resources/locale.pak"),
    b"first resource",
  )?;
  symlink("A", framework.join("Versions/Current"))?;
  symlink("Versions/Current/Resources", framework.join("Resources"))?;
  for suffix in [
    "Helper",
    "Helper (GPU)",
    "Helper (Renderer)",
    "Helper (Plugin)",
    "Helper (Alerts)",
  ] {
    let name = format!("{executable_name} {suffix}");
    let helper = path.join("Contents/Frameworks").join(format!("{name}.app"));
    executable(&helper.join("Contents/MacOS").join(&name), b"first helper")?;
    let mut helper_info = info.clone();
    helper_info.insert("CFBundleExecutable".into(), Value::String(name));
    Value::Dictionary(helper_info).to_file_xml(helper.join("Contents/Info.plist"))?;
  }
  Ok(())
}

pub fn source(output: &Path, client: &str) -> PathBuf {
  if client == "cli" {
    output
      .join(format!(
        "lyonbrown-cli-{VERSION}-macos-{}",
        std::env::consts::ARCH
      ))
      .join("Lyonbrown CLI.app")
  } else {
    output.join("Lyonbrown.app")
  }
}

pub fn fixture(root: &Path, name: &str, cli_only: bool) -> Result<(PathBuf, PathBuf)> {
  let output = root.join(name);
  let binaries = output.join("binaries");
  executable(&binaries.join("agentd"), DAEMON)?;
  bundle(&source(&output, "cli"), "cli")?;
  if !cli_only {
    bundle(&source(&output, "desktop"), "desktop")?;
  }
  Ok((binaries, output))
}

pub fn xml(path: &Path, expression: &str) -> Result<String> {
  let output = Command::new("/usr/bin/xmllint")
    .args(["--xpath", expression])
    .arg(path)
    .output()?;
  ensure!(
    output.status.success(),
    "XML inspection: {}",
    String::from_utf8_lossy(&output.stderr)
  );
  Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub fn leaves(path: &Path) -> Result<Vec<PathBuf>> {
  let mut leaves = Vec::new();
  for entry in WalkDir::new(path).follow_links(false) {
    let entry = entry?;
    if !entry.file_type().is_dir() {
      leaves.push(entry.path().strip_prefix(path)?.to_path_buf());
    }
  }
  leaves.sort();
  Ok(leaves)
}

pub fn check_nonrelocation(records: &[Value]) -> usize {
  records
    .iter()
    .map(|record| {
      let record = record.as_dictionary().expect("analyzed bundle dictionary");
      assert_eq!(record["BundleIsRelocatable"].as_boolean(), Some(false));
      1 + record.get("ChildBundles").map_or(0, |children| {
        check_nonrelocation(children.as_array().expect("child bundle array"))
      })
    })
    .sum()
}

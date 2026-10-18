#![cfg(target_os = "macos")]

#[path = "../../../packaging/src/install_bundle.rs"]
mod install_bundle;

use anyhow::Result;
use install_bundle::install_native_cli;
use std::{
  fs,
  os::unix::fs::{PermissionsExt, symlink},
  path::Path,
};

fn fixture(root: &Path) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
  let source = root.join("archive/bin");
  let bundle = root.join("archive/Lyonbrown CLI.app");
  fs::create_dir_all(&source)?;
  fs::create_dir_all(bundle.join("Contents/MacOS"))?;
  fs::write(bundle.join("Contents/MacOS/agent-cli"), b"unchanged client")?;
  fs::set_permissions(
    bundle.join("Contents/MacOS/agent-cli"),
    fs::Permissions::from_mode(0o755),
  )?;
  fs::create_dir_all(bundle.join("Contents/Frameworks/CEF.framework/Versions/A/Resources"))?;
  fs::write(
    bundle.join("Contents/Frameworks/CEF.framework/Versions/A/Resources/locale.pak"),
    b"first resource",
  )?;
  symlink(
    "A",
    bundle.join("Contents/Frameworks/CEF.framework/Versions/Current"),
  )?;
  symlink(
    "Versions/Current/Resources",
    bundle.join("Contents/Frameworks/CEF.framework/Resources"),
  )?;
  let destination = root.join("installed/bin");
  fs::create_dir_all(&destination)?;
  Ok((source, destination))
}

#[test]
fn resource_change_with_unchanged_client_retains_both_bundles() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let (source, destination) = fixture(temp.path())?;
  let version = "1.0.0".parse()?;
  install_native_cli(&source, &destination, &version)?;
  let first = fs::read_link(destination.join("agent"))?;
  let resource = "Contents/Frameworks/CEF.framework/Versions/A/Resources/locale.pak";
  fs::write(
    source
      .parent()
      .expect("archive")
      .join("Lyonbrown CLI.app")
      .join(resource),
    b"second resource",
  )?;
  install_native_cli(&source, &destination, &version)?;
  let second = fs::read_link(destination.join("agent"))?;
  assert_ne!(
    first, second,
    "resource-only updates must select a new retained bundle"
  );
  let first_bundle = first
    .parent()
    .and_then(Path::parent)
    .and_then(Path::parent)
    .expect("bundle");
  assert_eq!(fs::read(first_bundle.join(resource))?, b"first resource");
  let helper = source
    .parent()
    .expect("archive")
    .join("Lyonbrown CLI.app/Contents/MacOS/helper");
  fs::write(&helper, b"new helper")?;
  fs::set_permissions(&helper, fs::Permissions::from_mode(0o755))?;
  install_native_cli(&source, &destination, &version)?;
  assert_ne!(second, fs::read_link(destination.join("agent"))?);
  assert_eq!(fs::read(destination.join("agent"))?, b"unchanged client");
  Ok(())
}

#[test]
fn repeat_install_preserves_cef_links_and_refuses_tampered_retained_app() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let (source, destination) = fixture(temp.path())?;
  let version = "1.0.0".parse()?;
  install_native_cli(&source, &destination, &version)?;
  let first = fs::read_link(destination.join("agent"))?;
  install_native_cli(&source, &destination, &version)?;
  assert_eq!(fs::read_link(destination.join("agent"))?, first);
  let bundle = first
    .parent()
    .and_then(Path::parent)
    .and_then(Path::parent)
    .expect("bundle");
  assert_eq!(
    fs::read_link(bundle.join("Contents/Frameworks/CEF.framework/Versions/Current"))?,
    Path::new("A")
  );
  assert_eq!(
    fs::read(bundle.join("Contents/Frameworks/CEF.framework/Resources/locale.pak"))?,
    b"first resource"
  );
  let identity_directory = bundle
    .parent()
    .expect("identity")
    .file_name()
    .expect("name")
    .to_string_lossy();
  assert_eq!(
    identity_directory
      .strip_prefix("1.0.0-")
      .expect("version")
      .len(),
    64
  );
  fs::write(bundle.join("Contents/MacOS/agent-cli"), b"tampered")?;
  let error =
    install_native_cli(&source, &destination, &version).expect_err("corruption must fail");
  assert!(error.to_string().contains("corrupt"));
  assert_eq!(fs::read_link(destination.join("agent"))?, first);
  assert_eq!(
    fs::read(&first)?,
    b"tampered",
    "do not overwrite a retained running version"
  );
  Ok(())
}

#[test]
fn copy_and_activation_failures_leave_previous_command_usable() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let (source, destination) = fixture(temp.path())?;
  let version = "1.0.0".parse()?;
  install_native_cli(&source, &destination, &version)?;
  let first = fs::read_link(destination.join("agent"))?;
  let bundle = source.parent().expect("archive").join("Lyonbrown CLI.app");
  let executable = bundle.join("Contents/MacOS/agent-cli");
  fs::set_permissions(&executable, fs::Permissions::from_mode(0o644))?;
  let result = install_native_cli(&source, &destination, &version);
  fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
  assert!(
    result
      .expect_err("non-executable client must fail")
      .to_string()
      .contains("execute")
  );
  assert_eq!(fs::read_link(destination.join("agent"))?, first);
  assert_eq!(fs::read(destination.join("agent"))?, b"unchanged client");
  fs::write(bundle.join("Contents/MacOS/helper"), b"new helper")?;
  let result = install_bundle::install_with_copy(&source, &destination, &version, |_, _| {
    anyhow::bail!("injected copy failure")
  });
  assert!(result.is_err());
  assert_eq!(fs::read_link(destination.join("agent"))?, first);
  assert_eq!(fs::read(destination.join("agent"))?, b"unchanged client");
  // A successful copy with altered contents must also fail before activation.
  let result =
    install_bundle::install_with_copy(&source, &destination, &version, |input, output| {
      let status = std::process::Command::new("ditto")
        .arg(input)
        .arg(output)
        .status()?;
      assert!(status.success());
      fs::write(output.join("Contents/MacOS/helper"), b"changed during copy")?;
      Ok(())
    });
  assert!(
    result
      .expect_err("copied content mismatch")
      .to_string()
      .contains("differs")
  );
  assert_eq!(fs::read_link(destination.join("agent"))?, first);
  fs::set_permissions(&destination, fs::Permissions::from_mode(0o555))?;
  let result = install_native_cli(&source, &destination, &version);
  fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
  assert!(
    result.is_err(),
    "read-only command directory must reject activation"
  );
  assert_eq!(fs::read_link(destination.join("agent"))?, first);
  assert_eq!(fs::read(destination.join("agent"))?, b"unchanged client");
  Ok(())
}

#[test]
fn relative_destination_installs_a_resolving_command_link() -> Result<()> {
  let cwd = std::env::current_dir()?;
  let temp = tempfile::tempdir_in(&cwd)?;
  let (source, destination) = fixture(temp.path())?;
  let relative = destination.strip_prefix(&cwd)?;
  assert!(relative.is_relative());
  install_native_cli(&source, relative, &"1.0.0".parse()?)?;
  assert!(fs::read_link(destination.join("agent"))?.is_absolute());
  assert_eq!(fs::read(destination.join("agent"))?, b"unchanged client");
  Ok(())
}

#![cfg(unix)]

use agent_launch::bundle_identity::bundle_identity;
use anyhow::Result;
use std::{
  fs,
  os::unix::fs::{PermissionsExt, symlink},
};

#[test]
fn identity_covers_paths_modes_and_empty_directories_with_stable_order() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let root = temp.path().join("app");
  fs::create_dir(&root)?;
  fs::write(root.join("client"), b"client")?;
  let initial = bundle_identity(&root)?;
  assert_eq!(initial, bundle_identity(&root)?);
  fs::set_permissions(root.join("client"), fs::Permissions::from_mode(0o755))?;
  let executable = bundle_identity(&root)?;
  assert_ne!(initial, executable);
  fs::rename(root.join("client"), root.join("renamed-client"))?;
  let renamed = bundle_identity(&root)?;
  assert_ne!(executable, renamed);
  fs::create_dir(root.join("empty"))?;
  assert_ne!(renamed, bundle_identity(&root)?);
  Ok(())
}

#[test]
fn rejects_linked_roots_escaping_links_and_special_files() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let root = temp.path().join("app");
  fs::create_dir(&root)?;
  fs::write(temp.path().join("outside"), b"outside")?;
  symlink(&root, temp.path().join("linked-root"))?;
  assert!(bundle_identity(&temp.path().join("linked-root")).is_err());
  for target in [
    temp.path().join("outside"),
    "../outside".into(),
    "missing".into(),
  ] {
    symlink(target, root.join("link"))?;
    assert!(bundle_identity(&root).is_err());
    fs::remove_file(root.join("link"))?;
  }
  let status = std::process::Command::new("mkfifo")
    .arg(root.join("fifo"))
    .status()?;
  assert!(status.success());
  assert!(
    bundle_identity(&root)
      .expect_err("special file")
      .to_string()
      .contains("special file")
  );
  Ok(())
}

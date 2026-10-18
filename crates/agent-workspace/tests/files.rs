use agent_workspace::{ArtifactStore, Workspace};

#[test]
fn revision_save_confines_paths_and_rejects_stale_content() {
  let directory = tempfile::tempdir().unwrap();
  let workspace = Workspace::open(directory.path()).unwrap();
  let first = workspace
    .write_revision("src/file.txt", "first", "")
    .unwrap();
  assert!(
    workspace
      .write_revision("src/file.txt", "lost update", "")
      .is_err()
  );
  assert_eq!(workspace.read("src/file.txt").unwrap(), "first");
  let next = workspace
    .write_revision("src/file.txt", "second", &first)
    .unwrap();
  assert_ne!(first, next);
  assert!(workspace.write_revision("../escape", "bad", "").is_err());
  assert!(workspace.read("../escape").is_err());
  assert!(workspace.list("../").is_err());
}

#[test]
fn artifacts_publish_complete_data_and_reject_corrupt_existing_digest() {
  let directory = tempfile::tempdir().unwrap();
  let store = ArtifactStore::open(directory.path(), "node").unwrap();
  let artifact = store.put(b"fixture", "text/plain").unwrap();
  assert_eq!(store.read(&artifact.digest).unwrap(), b"fixture");
  assert_eq!(
    store.put(b"fixture", "text/plain").unwrap().digest,
    artifact.digest
  );
  std::fs::write(directory.path().join(&artifact.digest), b"crash residue").unwrap();
  assert!(store.put(b"fixture", "text/plain").is_err());
  assert!(store.read(&artifact.digest).is_err());
  assert_eq!(
    std::fs::read_dir(directory.path()).unwrap().count(),
    1,
    "temporary file must be cleaned on error"
  );
}

#[cfg(unix)]
#[test]
fn symlink_outside_workspace_is_not_readable_or_listed() {
  let directory = tempfile::tempdir().unwrap();
  let outside = tempfile::tempdir().unwrap();
  std::fs::write(outside.path().join("secret"), "outside").unwrap();
  std::os::unix::fs::symlink(outside.path(), directory.path().join("link")).unwrap();
  let workspace = Workspace::open(directory.path()).unwrap();
  assert!(workspace.read("link/secret").is_err());
  assert!(workspace.write("link/secret", "overwrite").is_err());
  assert!(workspace.list("").unwrap().is_empty());
  assert_eq!(
    std::fs::read_to_string(outside.path().join("secret")).unwrap(),
    "outside"
  );
}

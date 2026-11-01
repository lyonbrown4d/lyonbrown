use std::fs;

use agent_secrets::{ExposeSecret, MAX_SECRET_BYTES, SecretError, SecretStore, SecretString};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn round_trip_and_metadata_never_expose_plaintext() -> TestResult {
  let directory = tempfile::tempdir()?;
  let store = SecretStore::open(directory.path())?;
  let secret = SecretString::from("synthetic-provider-secret-for-storage");
  let receipt = store.put("provider-openai", &secret)?;
  assert_eq!(receipt.id, "provider-openai");
  assert_eq!(receipt.encryption_version, 1);
  let loaded = store.get("provider-openai")?.ok_or("missing secret")?;
  assert_eq!(loaded.expose_secret(), secret.expose_secret());
  let bytes = fs::read(directory.path().join("provider-openai.secret"))?;
  assert!(
    !bytes
      .windows(secret.expose_secret().len())
      .any(|window| { window == secret.expose_secret().as_bytes() })
  );
  let metadata = serde_json::to_string(&store.list()?)?;
  assert!(!metadata.contains(secret.expose_secret()));
  assert_eq!(store.list()?, vec![receipt]);
  assert!(!format!("{loaded:?}").contains(secret.expose_secret()));
  Ok(())
}

#[test]
fn reopening_and_update_preserve_identity_and_rotate_nonce() -> TestResult {
  let directory = tempfile::tempdir()?;
  let store = SecretStore::open(directory.path())?;
  let first = SecretString::from("synthetic-first-key");
  store.put("upstream", &first)?;
  let before = fs::read(directory.path().join("upstream.secret"))?;
  store.put("upstream", &first)?;
  let after = fs::read(directory.path().join("upstream.secret"))?;
  assert_ne!(&before[8..32], &after[8..32]);
  store.put("upstream", &SecretString::from("synthetic-updated-key"))?;
  drop(store);
  let reopened = SecretStore::open(directory.path())?;
  assert_eq!(
    reopened
      .get("upstream")?
      .ok_or("missing secret")?
      .expose_secret(),
    "synthetic-updated-key"
  );
  assert_eq!(reopened.list()?.len(), 1);
  assert!(fs::read_dir(directory.path())?.all(|entry| {
    entry.is_ok_and(|entry| !entry.file_name().to_string_lossy().starts_with(".pending-"))
  }));
  Ok(())
}

#[test]
fn tampering_fails_authentication_and_cannot_be_overwritten() -> TestResult {
  let directory = tempfile::tempdir()?;
  let store = SecretStore::open(directory.path())?;
  store.put("upstream", &SecretString::from("synthetic-original-key"))?;
  let path = directory.path().join("upstream.secret");
  let mut bytes = fs::read(&path)?;
  let index = bytes.len() - 1;
  bytes[index] ^= 1;
  fs::write(&path, &bytes)?;
  assert!(matches!(
    store.get("upstream"),
    Err(SecretError::Authentication)
  ));
  assert!(matches!(store.list(), Err(SecretError::Authentication)));
  assert!(matches!(
    store.put("upstream", &SecretString::from("replacement")),
    Err(SecretError::Authentication)
  ));
  assert_eq!(fs::read(path)?, bytes);
  Ok(())
}

#[test]
fn credential_identity_is_authenticated() -> TestResult {
  let directory = tempfile::tempdir()?;
  let store = SecretStore::open(directory.path())?;
  store.put("first", &SecretString::from("synthetic-key"))?;
  fs::copy(
    directory.path().join("first.secret"),
    directory.path().join("second.secret"),
  )?;
  assert!(matches!(
    store.get("second"),
    Err(SecretError::Authentication)
  ));
  Ok(())
}

#[test]
fn wrong_master_key_cannot_decrypt_existing_records() -> TestResult {
  let directory = tempfile::tempdir()?;
  let store = SecretStore::open(directory.path())?;
  store.put("upstream", &SecretString::from("synthetic-key"))?;
  drop(store);
  let path = directory.path().join("master.key");
  let mut bytes = fs::read(&path)?;
  bytes[0] ^= 1;
  fs::write(path, bytes)?;
  let reopened = SecretStore::open(directory.path())?;
  assert!(matches!(
    reopened.get("upstream"),
    Err(SecretError::Authentication)
  ));
  Ok(())
}

#[test]
fn missing_master_key_is_not_silently_recreated() -> TestResult {
  let directory = tempfile::tempdir()?;
  let store = SecretStore::open(directory.path())?;
  store.put("upstream", &SecretString::from("synthetic-key"))?;
  drop(store);
  fs::remove_file(directory.path().join("master.key"))?;
  assert!(matches!(
    SecretStore::open(directory.path()),
    Err(SecretError::InvalidKey)
  ));
  assert!(!directory.path().join("master.key").exists());
  Ok(())
}

#[test]
fn remove_is_scoped_and_reports_absence() -> TestResult {
  let directory = tempfile::tempdir()?;
  let store = SecretStore::open(directory.path())?;
  store.put("first", &SecretString::from("synthetic-first-key"))?;
  store.put("second", &SecretString::from("synthetic-second-key"))?;
  assert!(store.remove("first")?);
  assert!(!store.remove("first")?);
  assert!(store.get("first")?.is_none());
  assert_eq!(
    store
      .get("second")?
      .ok_or("missing second")?
      .expose_secret(),
    "synthetic-second-key"
  );
  assert_eq!(store.list()?.len(), 1);
  Ok(())
}

#[test]
fn rejects_unsafe_identifiers_and_invalid_values_without_writing() -> TestResult {
  let directory = tempfile::tempdir()?;
  let store = SecretStore::open(directory.path())?;
  for id in [
    "",
    "../outside",
    "absolute/path",
    "a\\b",
    ".hidden",
    "a:b",
    "UPSTREAM",
    "con",
    "lpt1",
    "白",
  ] {
    assert!(matches!(
      store.put(id, &SecretString::from("key")),
      Err(SecretError::InvalidId)
    ));
    assert!(matches!(store.get(id), Err(SecretError::InvalidId)));
    assert!(matches!(store.remove(id), Err(SecretError::InvalidId)));
  }
  for value in [
    "".into(),
    " \n".into(),
    "key\0tail".into(),
    "k".repeat(MAX_SECRET_BYTES + 1),
  ] {
    assert!(matches!(
      store.put("valid", &SecretString::from(value)),
      Err(SecretError::InvalidSecret)
    ));
  }
  assert!(store.list()?.is_empty());
  assert!(!directory.path().join("valid.secret").exists());
  Ok(())
}

#[cfg(unix)]
#[test]
fn private_modes_are_applied_and_public_key_file_is_rejected() -> TestResult {
  use std::os::unix::fs::PermissionsExt;

  let directory = tempfile::tempdir()?;
  let root = directory.path().join("credentials");
  let store = SecretStore::open(&root)?;
  store.put("upstream", &SecretString::from("synthetic-key"))?;
  assert_eq!(fs::metadata(&root)?.permissions().mode() & 0o777, 0o700);
  for name in [".lock", "master.key", "upstream.secret"] {
    assert_eq!(
      fs::metadata(root.join(name))?.permissions().mode() & 0o777,
      0o600
    );
  }
  drop(store);
  fs::set_permissions(root.join("master.key"), fs::Permissions::from_mode(0o644))?;
  assert!(matches!(
    SecretStore::open(root),
    Err(SecretError::Permissions)
  ));
  Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_records_and_root_are_rejected_without_touching_target() -> TestResult {
  use std::os::unix::fs::symlink;

  let directory = tempfile::tempdir()?;
  let target = directory.path().join("target");
  fs::create_dir(&target)?;
  symlink(&target, directory.path().join("linked-root"))?;
  assert!(matches!(
    SecretStore::open(directory.path().join("linked-root")),
    Err(SecretError::UnsupportedEntry)
  ));
  let store = SecretStore::open(directory.path().join("store"))?;
  let outside = directory.path().join("outside");
  fs::write(&outside, "unchanged")?;
  symlink(&outside, directory.path().join("store/upstream.secret"))?;
  assert!(matches!(
    store.get("upstream"),
    Err(SecretError::UnsupportedEntry)
  ));
  assert!(matches!(
    store.put("upstream", &SecretString::from("key")),
    Err(SecretError::UnsupportedEntry)
  ));
  assert_eq!(fs::read_to_string(outside)?, "unchanged");
  Ok(())
}

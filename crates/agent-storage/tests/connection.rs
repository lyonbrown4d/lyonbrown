use agent_storage::Store;

#[tokio::test]
async fn database_url_and_clone_share_durable_state_and_migration_ledger() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let url = format!("sqlite://{}?mode=rwc", path.display());
  let store = Store::connect(&url).await?;
  let session = store.create_session("shared", "/fixture", "local").await?;
  let cloned = store.clone();
  let (run, _) = cloned.accept_run(&session.id, 1, "hello", "once").await?;
  let snapshot = vec![7; 1024 * 1024];
  cloned
    .commit_snapshot(
      &session.id,
      1,
      &snapshot,
      "checkpoint",
      serde_json::json!({}),
    )
    .await?;
  drop(store);
  drop(cloned);
  let reopened = Store::connect(&url).await?;
  assert_eq!(reopened.load_snapshot(&session.id).await?, Some(snapshot));
  assert_eq!(
    reopened.run(&run.id).await?.expect("run").status,
    "accepted"
  );
  assert_eq!(reopened.events(&run.id, 0, 100).await?.len(), 2);
  assert_eq!(reopened.pending_outbox(100).await?.len(), 2);
  Ok(())
}

#[tokio::test]
async fn filesystem_paths_keep_url_delimiters_and_unicode_literal() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state?# 数据.sqlite");
  let store = Store::open(&path).await?;
  let session = store
    .create_session("literal path", "/fixture", "local")
    .await?;
  drop(store);
  assert!(path.is_file());
  assert!(!directory.path().join("state").exists());
  let reopened = Store::open(&path).await?;
  assert_eq!(
    reopened.session(&session.id).await?.expect("session").title,
    "literal path"
  );
  Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn unsupported_non_utf8_paths_fail_without_creating_a_lossy_filename() -> anyhow::Result<()> {
  use std::os::unix::ffi::OsStringExt;
  let directory = tempfile::tempdir()?;
  let path = directory
    .path()
    .join(std::ffi::OsString::from_vec(b"state-\xff.sqlite".to_vec()));
  let error = match Store::open(&path).await {
    Ok(_) => anyhow::bail!("SQLx requires UTF-8 filenames"),
    Err(error) => error,
  };
  assert!(agent_storage::is_database_error(&error));
  assert!(!path.exists());
  assert!(!std::path::Path::new(&path.to_string_lossy().into_owned()).exists());
  Ok(())
}

#[test]
fn wrapped_driver_errors_have_fixed_safe_diagnostics() {
  let error = anyhow::Error::new(sea_orm::DbErr::Custom(
    "SELECT secret FROM account; postgres://user:password@host/db".into(),
  ))
  .context("raw connection failure");
  assert!(agent_storage::is_database_error(&error));
  assert_eq!(
    agent_storage::safe_storage_error(&error),
    "database operation unavailable"
  );
  assert_eq!(
    agent_storage::safe_storage_error(&anyhow::anyhow!("private input")),
    "storage operation failed"
  );
}

#[test]
fn public_messages_preserve_validation_but_redact_wrapped_json_values() {
  let json =
    serde_json::from_str::<u64>("\"private stored payload\"").expect_err("integer expected");
  let error = anyhow::Error::new(json).context("decode record containing private content");
  assert_eq!(
    agent_storage::public_error_message(&error),
    "stored data is invalid"
  );
  let validation = anyhow::anyhow!("stale session owner epoch");
  assert_eq!(
    agent_storage::public_error_message(&validation),
    "stale session owner epoch"
  );
}

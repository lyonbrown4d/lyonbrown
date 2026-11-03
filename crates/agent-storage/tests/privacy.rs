use agent_storage::Store;
use anyhow::Result;

#[tokio::test]
async fn private_scope_write_requires_current_owner_and_survives_reopen() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let session = store.create_session("scope", "/fixture", "node").await?;
  assert!(store.load_private_scope(&session.id).await?.is_none());
  store
    .commit_private_scope(&session.id, session.owner_epoch, b"ciphertext-one")
    .await?;
  assert!(
    store
      .commit_private_scope(&session.id, session.owner_epoch + 1, b"wrong-owner")
      .await
      .is_err()
  );
  assert!(
    store
      .commit_private_scope(&session.id, session.owner_epoch, &vec![1; 65_537])
      .await
      .is_err()
  );
  drop(store);
  let store = Store::open(&path).await?;
  assert_eq!(
    store.load_private_scope(&session.id).await?,
    Some(b"ciphertext-one".to_vec())
  );
  Ok(())
}

use agent_core::{new_id, now_ms};
use agent_storage::Store;
use memory_core::*;

fn descriptor(revision: &str) -> EmbeddingDescriptor {
  EmbeddingDescriptor {
    id: "fixture".into(),
    model: "synthetic".into(),
    dimensions: 2,
    configuration_revision: revision.into(),
  }
}
fn mutation(id: &str, scope: MemoryScope) -> MemoryMutation {
  MemoryMutation {
    item: MemoryItem {
      id: id.into(),
      scope,
      kind: MemoryKind::Fact,
      state: MemoryState::Active,
      content: "synthetic fixture memory".into(),
      revision: 1,
      created_at_ms: 1,
      updated_at_ms: now_ms(),
      expires_at_ms: None,
    },
    expected_revision: 0,
    source: MemorySource::Explicit {
      request_id: new_id(),
    },
  }
}
#[tokio::test]
async fn rebuild_cursor_and_jobs_survive_restart_and_stay_scope_bounded() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("rebuild.sqlite");
  let url = path.to_string_lossy();
  let store = Store::open(&url).await?;
  let scopes = [MemoryScope::User("owner".into())];
  for index in 0..205 {
    store
      .save(mutation(&format!("memory-{index:04}"), scopes[0].clone()))
      .await?;
  }
  store
    .save(mutation("foreign", MemoryScope::User("other".into())))
    .await?;
  let descriptor = descriptor("one");
  let id = new_id();
  let first = store
    .start_rebuild(&id, &scopes, &descriptor, true, now_ms())
    .await?;
  assert_eq!(first.scanned, 0);
  assert!(store.advance_rebuild(&descriptor, 100, now_ms()).await?);
  let page = store.list_rebuilds(&scopes, None, 10).await?;
  assert_eq!(page.items[0].scanned, 100);
  assert_eq!(page.items[0].enqueued, 100);
  assert_eq!(page.items[0].after_id.as_deref(), Some("memory-0099"));
  drop(store);
  let store = Store::open(&url).await?;
  assert!(store.advance_rebuild(&descriptor, 100, now_ms()).await?);
  assert!(store.advance_rebuild(&descriptor, 100, now_ms()).await?);
  let progress = store
    .list_rebuilds(&scopes, None, 10)
    .await?
    .items
    .remove(0);
  assert_eq!(progress.scanned, 205);
  assert_eq!(progress.enqueued, 205);
  assert!(progress.scan_finished);
  assert_eq!(progress.state, MemoryRebuildState::Pending);
  assert!(!store.advance_rebuild(&descriptor, 100, now_ms()).await?);
  assert_eq!(store.jobs("foreign").await?.len(), 1);
  assert_eq!(store.jobs("memory-0000").await?.len(), 2);
  assert!(
    store
      .list_rebuilds(&[MemoryScope::User("other".into())], None, 10)
      .await?
      .items
      .is_empty()
  );
  // Complete the persisted child jobs, not just the scanning phase.
  while let Some(job) = store.claim_job().await? {
    store
      .finish_job(&job.id, job.attempts, MemoryJobState::Completed, None)
      .await?;
  }
  assert!(store.advance_rebuild(&descriptor, 100, now_ms()).await?);
  let complete = store
    .list_rebuilds(&scopes, None, 10)
    .await?
    .items
    .remove(0);
  assert_eq!(complete.state, MemoryRebuildState::Completed);
  Ok(())
}
#[tokio::test]
async fn generation_change_rebuilds_again_and_failed_child_requires_explicit_retry()
-> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let scopes = [MemoryScope::Session("session".into())];
  store.save(mutation("memory", scopes[0].clone())).await?;
  let old = descriptor("one");
  let first = store
    .start_rebuild(&new_id(), &scopes, &old, false, now_ms())
    .await?;
  let duplicate = store
    .start_rebuild(&new_id(), &scopes, &old, false, now_ms())
    .await?;
  assert_eq!(first.id, duplicate.id);
  assert!(store.advance_rebuild(&old, 10, now_ms()).await?);
  let mut failed = None;
  while let Some(job) = store.claim_job().await? {
    if failed.is_none() {
      store
        .finish_job(&job.id, job.attempts, MemoryJobState::Completed, None)
        .await?;
      failed = Some(String::new());
    } else {
      store
        .finish_job(
          &job.id,
          job.attempts,
          MemoryJobState::Uncertain,
          Some("fixture_interrupted"),
        )
        .await?;
      failed = Some(job.id);
    }
  }
  assert!(!store.advance_rebuild(&old, 10, now_ms()).await?);
  assert_eq!(
    store.list_rebuilds(&scopes, None, 10).await?.items[0].state,
    MemoryRebuildState::Pending
  );
  let failed = failed
    .filter(|id| !id.is_empty())
    .expect("fixture has a rebuild child");
  assert!(matches!(
    store
      .retry_job_for(&failed, Some(&descriptor("wrong-generation")))
      .await,
    Err(MemoryError::EmbeddingMismatch)
  ));
  assert_eq!(
    store
      .jobs("memory")
      .await?
      .iter()
      .find(|job| job.id == failed)
      .unwrap()
      .state,
    MemoryJobState::Uncertain
  );
  store.retry_job_for(&failed, Some(&old)).await?;
  let child = store.claim_job().await?.expect("explicitly retried child");
  assert_eq!(child.id, failed);
  store
    .finish_job(&child.id, child.attempts, MemoryJobState::Completed, None)
    .await?;
  assert!(store.advance_rebuild(&old, 10, now_ms()).await?);
  let new = descriptor("two");
  let changed = store
    .start_rebuild(&new_id(), &scopes, &new, false, now_ms())
    .await?;
  assert_ne!(changed.id, first.id);
  assert_eq!(changed.embedding.configuration_revision, "two");
  assert!(store.advance_rebuild(&new, 10, now_ms()).await?);
  assert_eq!(store.jobs("memory").await?.len(), 3);
  assert!(
    store.claim_job_for(&old).await?.is_none(),
    "an old embedding must not claim a new generation's child"
  );
  let new_child = store
    .claim_job_for(&new)
    .await?
    .expect("new generation child");
  store
    .finish_job(
      &new_child.id,
      new_child.attempts,
      MemoryJobState::Completed,
      None,
    )
    .await?;
  store.advance_rebuild(&new, 10, now_ms()).await?;
  let returned_old = store
    .start_rebuild(&new_id(), &scopes, &old, false, now_ms())
    .await?;
  assert_ne!(returned_old.id, first.id, "A to B to A must scan again");
  assert!(store.advance_rebuild(&old, 10, now_ms()).await?);

  assert!(matches!(
    store
      .start_rebuild(&first.id, &scopes, &new, true, now_ms())
      .await,
    Err(MemoryError::Conflict)
  ));
  Ok(())
}

#[tokio::test]
async fn new_explicit_rebuild_progresses_without_replaying_older_unknown_children()
-> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let scopes = [MemoryScope::User("owner".into())];
  store.save(mutation("memory", scopes[0].clone())).await?;
  let descriptor = descriptor("same-generation");
  let old = store
    .start_rebuild(&new_id(), &scopes, &descriptor, true, now_ms())
    .await?;
  store.advance_rebuild(&descriptor, 100, now_ms()).await?;
  let ordinary = store.claim_job().await?.expect("ordinary mutation job");
  store
    .finish_job(
      &ordinary.id,
      ordinary.attempts,
      MemoryJobState::Completed,
      None,
    )
    .await?;
  let unknown = store.claim_job().await?.expect("old rebuild job");
  store
    .finish_job(
      &unknown.id,
      unknown.attempts,
      MemoryJobState::Uncertain,
      Some("fixture_disconnect"),
    )
    .await?;
  let newer = store
    .start_rebuild(&new_id(), &scopes, &descriptor, true, now_ms())
    .await?;
  assert!(store.advance_rebuild(&descriptor, 100, now_ms()).await?);
  let page = store.list_rebuilds(&scopes, None, 10).await?;
  assert_eq!(
    page
      .items
      .iter()
      .find(|item| item.id == old.id)
      .unwrap()
      .state,
    MemoryRebuildState::Pending
  );
  assert_eq!(
    page
      .items
      .iter()
      .find(|item| item.id == newer.id)
      .unwrap()
      .scanned,
    1
  );
  let jobs = store.jobs("memory").await?;
  assert_eq!(jobs.len(), 3);
  assert_eq!(
    jobs.iter().find(|job| job.id == unknown.id).unwrap().state,
    MemoryJobState::Uncertain
  );
  Ok(())
}

#[tokio::test]
async fn caller_uuid_order_does_not_replace_transactional_generation_order() -> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let scopes = [MemoryScope::User("owner".into())];
  let first = store
    .start_rebuild(
      "ffffffff-ffff-4fff-8fff-ffffffffffff",
      &scopes,
      &descriptor("one"),
      true,
      now_ms(),
    )
    .await?;
  let second = store
    .start_rebuild(&new_id(), &scopes, &descriptor("two"), false, now_ms())
    .await?;
  let repeated = store
    .start_rebuild(&new_id(), &scopes, &descriptor("two"), false, now_ms())
    .await?;
  assert_eq!(second.id, repeated.id);
  let page = store.list_rebuilds(&scopes, None, 1).await?;
  assert_eq!(page.items[0].id, second.id);
  let next = store
    .list_rebuilds(&scopes, page.next_cursor.as_deref(), 1)
    .await?;
  assert_eq!(next.items[0].id, first.id);
  assert!(next.next_cursor.is_none());
  Ok(())
}

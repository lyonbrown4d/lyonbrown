use agent_core::{RunStatus, new_id, now_ms};
use agent_storage::Store;
use memory_core::*;
use sea_orm::prelude::*;

fn mutation(id: &str, scope: MemoryScope, revision: u64) -> MemoryMutation {
  let now = now_ms();
  MemoryMutation {
    item: MemoryItem {
      id: id.into(),
      scope,
      kind: MemoryKind::Fact,
      state: MemoryState::Active,
      content: format!("private fact revision {revision}"),
      revision,
      created_at_ms: 1,
      updated_at_ms: now,
      expires_at_ms: None,
    },
    expected_revision: revision - 1,
    source: MemorySource::Explicit {
      request_id: new_id(),
    },
  }
}
mod revisions {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "memory_revisions")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub memory_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub revision: i64,
    pub record: String,
    pub source: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}

#[tokio::test]
async fn revisions_deduplicate_exact_requests_and_forget_scrubs_history() -> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let first = mutation("one", MemoryScope::User("u".into()), 1);
  assert_eq!(store.save(first.clone()).await?, first.item);
  assert_eq!(store.save(first.clone()).await?, first.item);
  let mut timestamp_retry = first.clone();
  timestamp_retry.item.created_at_ms += 1;
  timestamp_retry.item.updated_at_ms += 1;
  assert_eq!(store.save(timestamp_retry).await?, first.item);
  let mut changed = first.clone();
  changed.item.content = "different body".into();
  assert!(matches!(
    store.save(changed).await,
    Err(MemoryError::Conflict)
  ));
  let second = mutation("one", first.item.scope.clone(), 2);
  store.save(second.clone()).await?;
  assert!(matches!(
    store
      .save(mutation("one", first.item.scope.clone(), 2))
      .await,
    Err(MemoryError::Conflict)
  ));
  let deleted = store.forget("one", 2).await?;
  assert_eq!(deleted.state, MemoryState::Deleted);
  assert!(deleted.content.is_empty());
  assert_eq!(deleted.revision, 3);
  let history = revisions::Entity::find()
    .filter(revisions::Column::MemoryId.eq("one"))
    .all(store.connection())
    .await?;
  assert_eq!(history.len(), 3);
  for row in history {
    let item: MemoryItem = serde_json::from_str(&row.record)?;
    assert!(item.content.is_empty());
    assert_eq!(item.state, MemoryState::Deleted);
    assert!(row.source.is_empty());
  }
  assert!(matches!(
    store
      .save(mutation("one", first.item.scope.clone(), 4))
      .await,
    Err(MemoryError::Conflict)
  ));
  assert!(matches!(
    store.save(first).await,
    Err(MemoryError::Conflict)
  ));
  assert_eq!(store.jobs("one").await?.len(), 3);
  assert_eq!(store.jobs("one").await?[0].kind, MemoryJobKind::Delete);
  assert!(
    store
      .events("memory:one", 0, 100)
      .await?
      .iter()
      .all(|event| !event.payload_json.contains("private fact"))
  );
  Ok(())
}

#[tokio::test]
async fn scoped_state_pages_are_stable_and_claim_recovery_never_replays_unknown_jobs()
-> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let scope = MemoryScope::Project("/project".into());
  for id in ["c", "a", "b"] {
    store.save(mutation(id, scope.clone(), 1)).await?;
  }
  store
    .save(mutation("foreign", MemoryScope::User("u".into()), 1))
    .await?;
  let mut proposed = mutation("proposal", scope.clone(), 1);
  proposed.item.state = MemoryState::Proposed;
  store.save(proposed).await?;
  let page = store
    .list(
      std::slice::from_ref(&scope),
      &[MemoryState::Active],
      None,
      2,
    )
    .await?;
  assert_eq!(
    page
      .items
      .iter()
      .map(|item| item.id.as_str())
      .collect::<Vec<_>>(),
    ["a", "b"]
  );
  assert_eq!(page.next_cursor.as_deref(), Some("b"));
  let rest = store
    .list(
      &[scope],
      &[MemoryState::Active],
      page.next_cursor.as_deref(),
      2,
    )
    .await?;
  assert_eq!(rest.items[0].id, "c");
  assert!(rest.next_cursor.is_none());
  assert!(matches!(
    store.list(&[], &[], None, 101).await,
    Err(MemoryError::Invalid)
  ));
  let job = store.claim_job().await?.expect("pending job");
  assert_eq!(job.attempts, 1);
  assert_eq!(job.state, MemoryJobState::Running);
  assert_eq!(store.recover_jobs().await?, 1);
  assert_eq!(store.recover_jobs().await?, 0);
  assert!(matches!(
    store
      .finish_job(&job.id, job.attempts, MemoryJobState::Completed, None)
      .await,
    Err(MemoryError::Conflict)
  ));
  assert_eq!(
    store.jobs(&job.memory_id).await?[0].state,
    MemoryJobState::Uncertain
  );
  let next = store.claim_job().await?.expect("another pending job");
  assert_ne!(next.id, job.id);
  store
    .finish_job(&next.id, next.attempts, MemoryJobState::Completed, None)
    .await?;
  assert!(matches!(
    store
      .finish_job(
        &next.id,
        next.attempts,
        MemoryJobState::Failed,
        Some("failed")
      )
      .await,
    Err(MemoryError::Conflict)
  ));
  assert!(matches!(
    store.retry_job(&next.id).await,
    Err(MemoryError::Conflict)
  ));
  store.retry_job(&job.id).await?;
  store.retry_job(&job.id).await?;
  let retried = store.claim_job().await?.expect("explicit retry");
  assert_eq!(retried.id, job.id);
  assert_eq!(retried.attempts, 2);
  assert!(matches!(
    store
      .finish_job(&job.id, job.attempts, MemoryJobState::Completed, None)
      .await,
    Err(MemoryError::Conflict)
  ));
  assert_eq!(
    store.jobs(&job.memory_id).await?[0].state,
    MemoryJobState::Running
  );
  store
    .finish_job(
      &retried.id,
      retried.attempts,
      MemoryJobState::Completed,
      None,
    )
    .await?;

  assert!(matches!(
    store.retry_job(&retried.id).await,
    Err(MemoryError::Conflict)
  ));
  Ok(())
}

#[tokio::test]
async fn source_and_run_binding_validate_epoch_lifecycle_and_omit_memory_text() -> anyhow::Result<()>
{
  let store = Store::open(":memory:").await?;
  let session = store.create_session("memory", "/project", "local").await?;
  let (source, _) = store.accept_run(&session.id, 1, "source", "source").await?;
  store
    .update_run_owned(&source.id, 1, RunStatus::Running, "", "")
    .await?;
  let mut value = mutation("session-fact", MemoryScope::Session(session.id.clone()), 1);
  value.source = MemorySource::Session {
    session_id: session.id.clone(),
    run_id: source.id.clone(),
    event_cursor: 1,
    owner_epoch: 1,
  };
  assert!(matches!(
    store.save(value.clone()).await,
    Err(MemoryError::Forbidden)
  ));
  store
    .update_run_owned(&source.id, 1, RunStatus::Completed, "result", "")
    .await?;
  let item = store.save(value).await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "consume", "consume")
    .await?;
  store
    .bind_run(&run.id, &session.id, 1, std::slice::from_ref(&item))
    .await?;
  store
    .bind_run(&run.id, &session.id, 1, std::slice::from_ref(&item))
    .await?;
  let events = store.events(&run.id, 0, 100).await?;
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind == "memory_bound")
      .count(),
    1
  );
  assert!(
    events
      .iter()
      .all(|event| !event.payload_json.contains(&item.content))
  );
  assert!(matches!(
    store
      .bind_run(&run.id, &session.id, 2, std::slice::from_ref(&item))
      .await,
    Err(MemoryError::Forbidden)
  ));
  store.forget(&item.id, 1).await?;
  assert!(matches!(
    store.bind_run(&run.id, &session.id, 1, &[item]).await,
    Err(MemoryError::Conflict)
  ));
  Ok(())
}

#[tokio::test]
async fn memory_outbox_failure_rolls_back_cas_history_job_and_request_dedup() -> anyhow::Result<()>
{
  let store = Store::open(":memory:").await?;
  store.connection().execute_unprepared("CREATE TRIGGER reject_memory_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT, 'injected storage fault'); END;").await?;
  let value = mutation("rollback", MemoryScope::User("u".into()), 1);
  assert!(matches!(
    store.save(value.clone()).await,
    Err(MemoryError::Storage)
  ));
  assert!(store.get("rollback").await?.is_none());
  assert!(store.jobs("rollback").await?.is_empty());
  assert_eq!(
    revisions::Entity::find().count(store.connection()).await?,
    0
  );
  store
    .connection()
    .execute_unprepared("DROP TRIGGER reject_memory_outbox;")
    .await?;
  store.save(value).await?;
  assert_eq!(store.pending_outbox(100).await?.len(), 1);
  Ok(())
}

#[tokio::test]
async fn project_registry_has_stable_opaque_ids_and_isolates_owner_and_workspace()
-> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let directory = tempfile::tempdir()?;
  let workspace = directory
    .path()
    .canonicalize()?
    .to_string_lossy()
    .into_owned();
  let clone = store.clone();
  let (left, right) = tokio::join!(
    store.project_scope("owner_a", &workspace),
    clone.project_scope("owner_a", &workspace)
  );
  let scope = left?;
  assert_eq!(scope, right?);
  let MemoryScope::Project(id) = &scope else {
    anyhow::bail!("project scope expected");
  };
  uuid::Uuid::parse_str(id)?;
  assert_ne!(store.project_scope("owner_b", &workspace).await?, scope);
  let moved = directory.path().join("moved");
  let moved = moved.to_string_lossy();
  assert_ne!(store.project_scope("owner_a", &moved).await?, scope);
  assert_eq!(store.project_scope("owner_a", &workspace).await?, scope);
  assert!(matches!(
    store.project_scope("owner_a", "relative/path").await,
    Err(MemoryError::Invalid)
  ));
  let events = store
    .events(&format!("memory_project:{id}"), 0, 100)
    .await?;
  assert_eq!(events.len(), 1);
  assert!(!events[0].payload_json.contains(&workspace));
  Ok(())
}

#[tokio::test]
async fn repository_fences_terminal_reactivation_and_model_demotion_inside_cas_transaction()
-> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let session = store.create_session("source", "/project", "local").await?;
  let (run, _) = store.accept_run(&session.id, 1, "source", "source").await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Completed, "done", "")
    .await?;
  let scope = MemoryScope::Session(session.id.clone());
  let mut rejected = mutation("terminal", scope.clone(), 1);
  rejected.item.state = MemoryState::Rejected;
  store.save(rejected).await?;
  assert!(matches!(
    store.save(mutation("terminal", scope.clone(), 2)).await,
    Err(MemoryError::Conflict)
  ));
  store.save(mutation("active", scope.clone(), 1)).await?;
  let mut proposal = mutation("active", scope, 2);
  proposal.item.state = MemoryState::Proposed;
  proposal.source = MemorySource::Session {
    session_id: session.id,
    run_id: run.id,
    event_cursor: 1,
    owner_epoch: 1,
  };
  assert!(matches!(
    store.save(proposal).await,
    Err(MemoryError::Conflict)
  ));
  assert_eq!(
    store.get("active").await?.expect("memory").state,
    MemoryState::Active
  );
  assert_eq!(store.jobs("active").await?.len(), 1);
  assert_eq!(store.events("memory:active", 0, 100).await?.len(), 1);
  Ok(())
}

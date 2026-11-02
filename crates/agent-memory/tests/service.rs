mod common;
use agent_memory::{MemoryAccess, MemoryService};
use common::*;
use memory_core::*;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn retrieval_passes_exact_scopes_and_filters_stale_foreign_deleted_expired_and_nonfinite_hits()
-> MemoryResult<()> {
  let repo = Arc::new(Repository::default());
  let index = Arc::new(Index::default());
  let scope = MemoryScope::User("owner".into());
  let access = MemoryAccess::new(vec![scope.clone()]);
  let good = item(scope.clone());
  let stale = item(scope.clone());
  let foreign = item(MemoryScope::User("other".into()));
  let mut deleted = item(scope.clone());
  deleted.state = MemoryState::Deleted;
  let mut expired = item(scope);
  expired.expires_at_ms = Some(0);
  for candidate in [&good, &stale, &foreign, &deleted, &expired] {
    repo.put(candidate.clone());
  }
  *index.hits.lock().expect("hits") = vec![
    VectorHit {
      id: foreign.id.clone(),
      revision: 1,
      score: 1.0,
    },
    VectorHit {
      id: stale.id.clone(),
      revision: 0,
      score: 0.99,
    },
    VectorHit {
      id: deleted.id.clone(),
      revision: 1,
      score: 0.98,
    },
    VectorHit {
      id: expired.id.clone(),
      revision: 1,
      score: 0.97,
    },
    VectorHit {
      id: good.id.clone(),
      revision: 1,
      score: f32::NAN,
    },
    VectorHit {
      id: good.id.clone(),
      revision: 1,
      score: 0.9,
    },
  ];
  let service = MemoryService::new(
    repo,
    Some(Arc::new(Embedding {
      cancel: false,
      revise: None,
    })),
    Some(index.clone()),
  )?;
  let results = service
    .search(&access, "fixture query", 2, CancellationToken::new())
    .await?;
  assert_eq!(results.len(), 1);
  assert_eq!(results[0].item, good);
  assert_eq!(results[0].score, 0.9);
  let queries = index.queries.lock().expect("queries");
  assert_eq!(queries[0].scopes.as_slice(), access.scopes());
  assert_eq!(queries[0].limit, 8);
  Ok(())
}

#[tokio::test]
async fn mutations_enforce_scope_human_activation_terminal_state_and_model_proposal_boundaries()
-> MemoryResult<()> {
  let repo = Arc::new(Repository::default());
  let service = MemoryService::new(repo.clone(), None, None)?;
  let scope = MemoryScope::User("owner".into());
  let access = MemoryAccess::new(vec![scope.clone()]);
  assert!(matches!(
    service
      .save(&access, mutation(item(MemoryScope::User("other".into()))))
      .await,
    Err(MemoryError::Forbidden)
  ));
  let mut request = mutation(item(scope));
  request.source = MemorySource::Session {
    session_id: "session".into(),
    run_id: "run".into(),
    event_cursor: 1,
    owner_epoch: 1,
  };
  assert!(matches!(
    service.save(&access, request.clone()).await,
    Err(MemoryError::Forbidden)
  ));
  let proposed = service.propose(&access, request.clone()).await?;
  assert_eq!(proposed.state, MemoryState::Proposed);
  request.item = proposed.clone();
  request.item.revision = 2;
  request.item.state = MemoryState::Active;
  request.expected_revision = 1;
  request.source = MemorySource::Explicit {
    request_id: uuid::Uuid::new_v4().to_string(),
  };
  let active = service.save(&access, request.clone()).await?;
  assert_eq!(active.state, MemoryState::Active);
  let mut agent_edit = request.clone();
  agent_edit.item.revision = 3;
  agent_edit.expected_revision = 2;
  agent_edit.source = MemorySource::Session {
    session_id: "session".into(),
    run_id: "run".into(),
    event_cursor: 1,
    owner_epoch: 1,
  };
  assert!(matches!(
    service.propose(&access, agent_edit).await,
    Err(MemoryError::Conflict)
  ));
  let mut terminal = active;
  terminal.state = MemoryState::Rejected;
  repo.put(terminal);
  request.item.revision = 3;
  request.expected_revision = 2;
  assert!(matches!(
    service.save(&access, request).await,
    Err(MemoryError::Conflict)
  ));
  assert_eq!(repo.saved.lock().expect("saved").len(), 2);
  assert!(!service.status().await?.retrieval_ready);
  assert!(matches!(
    service
      .search(&access, "query", 1, CancellationToken::new())
      .await,
    Err(MemoryError::RetrievalUnavailable)
  ));
  Ok(())
}

#[tokio::test]
async fn worker_revalidates_after_embedding_and_deletes_only_through_the_intended_revision()
-> MemoryResult<()> {
  let repo = Arc::new(Repository::default());
  let index = Arc::new(Index::default());
  let value = item(MemoryScope::User("owner".into()));
  repo.put(value.clone());
  repo.queue(&value, MemoryJobKind::Upsert);
  let service = MemoryService::new(
    repo.clone(),
    Some(Arc::new(Embedding {
      cancel: false,
      revise: Some(repo.clone()),
    })),
    Some(index.clone()),
  )?;
  assert!(service.process_one(CancellationToken::new()).await?);
  assert!(index.upserts.lock().expect("upserts").is_empty());
  repo.queue(&value, MemoryJobKind::Delete);
  assert!(service.process_one(CancellationToken::new()).await?);
  assert_eq!(*index.deletes.lock().expect("deletes"), vec![(value.id, 1)]);
  assert_eq!(
    repo.job.lock().expect("job").as_ref().expect("job").state,
    MemoryJobState::Completed
  );
  Ok(())
}

#[tokio::test]
async fn unknown_mutations_stay_uncertain_without_automatic_replay_and_finish_uses_attempt_fence()
-> MemoryResult<()> {
  let repo = Arc::new(Repository::default());
  let value = item(MemoryScope::User("owner".into()));
  repo.put(value.clone());
  repo.queue(&value, MemoryJobKind::Upsert);
  let index = Arc::new(Index {
    uncertain: true,
    ..Default::default()
  });
  let service = MemoryService::new(
    repo.clone(),
    Some(Arc::new(Embedding {
      cancel: false,
      revise: None,
    })),
    Some(index.clone()),
  )?;
  assert!(service.process_one(CancellationToken::new()).await?);
  assert!(!service.process_one(CancellationToken::new()).await?);
  assert_eq!(
    repo.job.lock().expect("job").as_ref().expect("job").state,
    MemoryJobState::Uncertain
  );
  assert_eq!(index.upserts.lock().expect("upserts").len(), 1);
  let no_index = MemoryService::new(repo.clone(), None, None)?;
  repo.queue(&value, MemoryJobKind::Upsert);
  assert!(!no_index.process_one(CancellationToken::new()).await?);
  assert_eq!(
    repo.job.lock().expect("job").as_ref().expect("job").state,
    MemoryJobState::Pending
  );
  let late = Arc::new(Index {
    late_attempt: Some(repo.clone()),
    ..Default::default()
  });
  let service = MemoryService::new(
    repo.clone(),
    Some(Arc::new(Embedding {
      cancel: false,
      revise: None,
    })),
    Some(late),
  )?;
  assert!(matches!(
    service.process_one(CancellationToken::new()).await,
    Err(MemoryError::Conflict)
  ));
  assert_eq!(
    repo.job.lock().expect("job").as_ref().expect("job").state,
    MemoryJobState::Running
  );
  Ok(())
}

#[tokio::test]
async fn cancelled_work_is_uncertain_and_waits_for_an_explicit_retry() -> MemoryResult<()> {
  let repo = Arc::new(Repository::default());
  let value = item(MemoryScope::User("owner".into()));
  repo.put(value.clone());
  repo.queue(&value, MemoryJobKind::Upsert);
  let service = MemoryService::new(
    repo.clone(),
    Some(Arc::new(Embedding {
      cancel: true,
      revise: None,
    })),
    Some(Arc::new(Index::default())),
  )?;
  assert!(matches!(
    service.process_one(CancellationToken::new()).await,
    Err(MemoryError::Cancelled)
  ));
  assert_eq!(
    repo.job.lock().expect("job").as_ref().expect("job").state,
    MemoryJobState::Uncertain
  );
  assert!(!service.process_one(CancellationToken::new()).await?);
  Ok(())
}

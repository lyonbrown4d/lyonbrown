mod support;

use std::sync::atomic::Ordering;

use anyhow::Result;
use support::Node;

struct UnknownSideEffect;
#[async_trait::async_trait]
impl agent_distributed::worker::WorkerExecutor for UnknownSideEffect {
  async fn execute(
    &self,
    _: agent_distributed::worker::WorkerJob,
    cancellation: tokio_util::sync::CancellationToken,
  ) -> Result<String> {
    cancellation.cancel();
    Err(agent_distributed::worker::UncertainOutcome.into())
  }
}

#[tokio::test]
async fn executor_unknown_outcome_stays_uncertain_even_when_cancellation_was_requested()
-> Result<()> {
  let master = Node::new(false).await?;
  let mut worker = Node::new(false).await?;
  master.pair(&worker)?;
  worker.pair(&master)?;
  worker.worker = agent_distributed::worker::TaskWorker::new(
    worker.manager.clone(),
    std::sync::Arc::new(UnknownSideEffect),
    worker.directory.path().join("workspaces"),
  )?;
  let envelope = master.envelope();
  let (stop, server) = worker.serve()?;
  let mut client = master.client(&worker).await?;
  client.submit_task(envelope.clone()).await?;
  let task = tokio::time::timeout(std::time::Duration::from_secs(3), async {
    loop {
      let task = client
        .get_task(agent_protocol::v1::TaskRequest {
          task_id: envelope.task_id.clone(),
        })
        .await?
        .into_inner();
      if task.attempt_status != "accepted" && task.attempt_status != "running" {
        return Ok::<_, tonic::Status>(task);
      }
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await??;
  assert_eq!(task.status, "uncertain");
  assert_eq!(task.attempt_status, "uncertain");
  assert_eq!(
    worker
      .store()?
      .task(&envelope.task_id)?
      .expect("task")
      .status,
    "uncertain"
  );
  stop.cancel();
  server.await??;
  Ok(())
}

#[tokio::test]
async fn restart_marks_unfinished_task_uncertain_without_reexecution() -> Result<()> {
  let master = Node::new(false).await?;
  let mut worker = Node::new(false).await?;
  master.pair(&worker)?;
  worker.pair(&master)?;
  let envelope = master.envelope();
  let workspace = worker
    .directory
    .path()
    .join("workspaces")
    .join(&envelope.task_id)
    .join(&envelope.attempt_id);
  worker.store()?.accept_worker_task(
    envelope.clone().into(),
    &worker.identity.public.node_id,
    workspace.to_str().expect("test path"),
  )?;
  assert_eq!(worker.worker.recover_uncertain()?, 1);
  let (stop, server) = worker.serve()?;
  let mut client = master.client(&worker).await?;
  let task = client.submit_task(envelope).await?.into_inner();
  assert_eq!(task.status, "uncertain");
  assert_eq!(worker.executor.calls.load(Ordering::SeqCst), 0);
  stop.cancel();
  server.await??;
  Ok(())
}

#[tokio::test]
async fn failed_atomic_worker_acceptance_leaves_no_session_task_event_or_outbox() -> Result<()> {
  let master = Node::new(false).await?;
  let worker = Node::new(false).await?;
  let mut envelope = master.envelope();
  envelope.owner_epoch = u64::MAX;
  let store = worker.store()?;
  assert!(
    store
      .accept_worker_task(
        envelope.clone().into(),
        &worker.identity.public.node_id,
        "/synthetic/workspace"
      )
      .is_err()
  );
  assert!(store.tasks()?.is_empty());
  assert!(store.sessions()?.is_empty());
  assert!(store.events(&envelope.task_id, 0, 100)?.is_empty());
  assert!(store.pending_outbox(100)?.is_empty());
  Ok(())
}

#[tokio::test]
async fn master_ownership_change_rejects_stale_attempt_result() -> Result<()> {
  let master = Node::new(false).await?;
  let envelope = master.envelope();
  let store = master.store()?;
  store.accept_dispatched_task(envelope.clone().into(), "paired-worker")?;
  store.mark_task_running(
    &envelope.task_id,
    &envelope.attempt_id,
    envelope.owner_epoch,
  )?;
  store.change_owner(&envelope.session_id, envelope.owner_epoch, "new-master")?;
  assert!(
    store
      .finish_attempt(
        &envelope.task_id,
        &envelope.attempt_id,
        envelope.owner_epoch,
        "stale result",
        ""
      )
      .is_err()
  );
  assert_eq!(
    store.task(&envelope.task_id)?.expect("task").attempt_status,
    "running"
  );
  Ok(())
}

#[tokio::test]
async fn restart_recovers_only_local_worker_attempts_and_preserves_foreign_roles() -> Result<()> {
  for ownership_changed in [true, false] {
    let local = Node::new(false).await?;
    let remote = Node::new(false).await?;
    let master = local.envelope();
    let generic = local.envelope();
    let store = local.store()?;
    store.accept_master_dispatched_task(master.clone().into(), &remote.identity.public.node_id)?;
    store.mark_task_running(&master.task_id, &master.attempt_id, master.owner_epoch)?;
    store.accept_task(generic.clone().into())?;
    if ownership_changed {
      store.change_owner(&master.session_id, master.owner_epoch, "new-master")?;
    }
    let foreign_snapshot = || -> Result<serde_json::Value> {
      Ok(serde_json::json!({
        "tasks": [store.task(&master.task_id)?, store.task(&generic.task_id)?],
        "sessions": [store.session(&master.session_id)?, store.session(&generic.session_id)?],
        "events": [
          store.events(&master.task_id, 0, 100)?,
          store.events(&generic.task_id, 0, 100)?,
          store.events(&master.session_id, 0, 100)?,
          store.events(&generic.session_id, 0, 100)?,
        ],
        "outbox": store.pending_outbox(100)?.into_iter()
          .filter(|event| event.run_id == master.task_id || event.run_id == generic.task_id
            || event.run_id == master.session_id || event.run_id == generic.session_id)
          .collect::<Vec<_>>(),
      }))
    };
    let foreign_before = foreign_snapshot()?;
    let sessions_before = store.sessions()?;
    let outbox_before = store.pending_outbox(100)?;
    assert_eq!(local.worker.recover_uncertain()?, 0);
    assert_eq!(foreign_snapshot()?, foreign_before);
    assert_eq!(store.sessions()?, sessions_before);
    assert_eq!(store.pending_outbox(100)?, outbox_before);

    let worker_task = remote.envelope();
    let workspace = local.directory.path().join("local-worker-attempt");
    store.accept_worker_task(
      worker_task.clone().into(),
      &local.identity.public.node_id,
      workspace.to_str().expect("test workspace"),
    )?;
    let sessions_before = store.sessions()?;
    let task_events_before = store.events(&worker_task.task_id, 0, 100)?;
    let outbox_before = store.pending_outbox(100)?;
    assert_eq!(local.worker.recover_uncertain()?, 1);
    let recovered = local
      .store()?
      .task(&worker_task.task_id)?
      .expect("local worker task");
    assert_eq!(recovered.status, "uncertain");
    assert_eq!(recovered.attempt_status, "uncertain");
    assert_eq!(foreign_snapshot()?, foreign_before);
    assert_eq!(store.sessions()?, sessions_before);
    let task_events = store.events(&worker_task.task_id, 0, 100)?;
    assert_eq!(task_events.len(), task_events_before.len() + 1);
    assert_eq!(&task_events[..task_events_before.len()], task_events_before);
    let recovery_event = task_events.last().expect("recovery event");
    assert_eq!(recovery_event.kind, "task_updated");
    let outbox = store.pending_outbox(100)?;
    assert_eq!(outbox.len(), outbox_before.len() + 1);
    assert!(outbox.contains(recovery_event));
    assert!(outbox_before.iter().all(|event| outbox.contains(event)));
    assert_eq!(local.worker.recover_uncertain()?, 0);
    assert_eq!(local.executor.calls.load(Ordering::SeqCst), 0);
    if ownership_changed {
      assert!(
        store
          .finish_attempt(
            &master.task_id,
            &master.attempt_id,
            master.owner_epoch,
            "stale",
            ""
          )
          .is_err()
      );
      assert_eq!(foreign_snapshot()?, foreign_before);
      assert_eq!(store.pending_outbox(100)?, outbox);
    }
  }
  Ok(())
}

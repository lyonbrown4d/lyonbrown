use agent_protocol::v1;
use clap::Parser;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resolution_waits_for_detached_workspace_writer_before_recording_fence() {
  let data = tempfile::tempdir().unwrap();
  let workspace = tempfile::tempdir().unwrap();
  let mut config = crate::config::Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(data.path().into());
  let daemon = crate::Daemon::open(config).await.unwrap();
  let session = daemon
    .store
    .create_session(
      "unknown write",
      workspace.path().to_str().unwrap(),
      &daemon.node_id,
    )
    .unwrap();
  let (run, _) = daemon
    .store
    .accept_run(&session.id, 1, "interrupted write", "original")
    .unwrap();
  daemon
    .store
    .update_run_owned(
      &run.id,
      1,
      agent_core::RunStatus::Uncertain,
      "",
      "unknown effect",
    )
    .unwrap();
  let writer = daemon.workspace_writes.clone().lock_owned().await;
  let resolver = daemon.clone();
  let id = run.id.clone();
  let recovery = tokio::spawn(async move {
    resolver
      .resolve_run_recovery(v1::RecoverRunRequest {
        run_id: id,
        owner_epoch: 1,
        action: v1::RunRecoveryAction::Abandon.into(),
        idempotency_key: "checked-write".into(),
        note: "Human verified the final write".into(),
        prompt: String::new(),
      })
      .await
  });
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while daemon.submission.try_lock().is_ok() {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  assert!(daemon.store.run_recovery(&run.id).unwrap().is_none());
  assert_eq!(
    daemon
      .store
      .session(&session.id)
      .unwrap()
      .unwrap()
      .owner_epoch,
    1
  );
  // This mirrors the owned guard retained by a detached spawn_blocking writer.
  // Its side effect must finish before recovery commits the human decision.
  let marker = workspace.path().join("finished-write.txt");
  std::fs::write(&marker, "verified final bytes").unwrap();
  drop(writer);
  recovery.await.unwrap().unwrap();
  assert_eq!(
    std::fs::read_to_string(marker).unwrap(),
    "verified final bytes"
  );
  assert!(daemon.store.run_recovery(&run.id).unwrap().is_some());
  assert_eq!(
    daemon
      .store
      .session(&session.id)
      .unwrap()
      .unwrap()
      .owner_epoch,
    2
  );
  assert_eq!(
    daemon.store.run(&run.id).unwrap().unwrap().status,
    "uncertain"
  );
  daemon.shutdown().await;
}

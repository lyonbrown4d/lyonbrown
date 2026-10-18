mod support;

use std::{sync::atomic::Ordering, time::Duration};

use agent_distributed::dispatcher::Dispatcher;
use agent_protocol::v1;
use anyhow::Result;
use support::Node;

#[tokio::test]
async fn paired_nodes_execute_once_in_isolated_session_and_master_accepts_separately() -> Result<()>
{
  let mut master = Node::new(false).await?;
  let mut worker = Node::new(false).await?;
  master.pair(&worker)?;
  worker.pair(&master)?;
  let (master_stop, master_server) = master.serve()?;
  let (worker_stop, worker_server) = worker.serve()?;
  let dispatcher = Dispatcher::new(master.manager.clone());
  let envelope = master.envelope();
  let accepted = dispatcher
    .submit(&worker.manager.local_node(), envelope.clone().into())
    .await?;
  assert_ne!(accepted.status, "completed");
  let mut client = master.client(&worker).await?;
  let replay = client.submit_task(envelope.clone()).await?.into_inner();
  assert_eq!(
    replay.envelope.as_ref().map(|value| value.task_id.as_str()),
    Some(envelope.task_id.as_str())
  );
  let finished = tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      let value = dispatcher
        .reconcile(&worker.manager.local_node(), &envelope.task_id)
        .await?;
      if value.attempt_status == "succeeded" {
        break Ok::<_, anyhow::Error>(value);
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await??;
  assert_eq!(finished.status, "awaiting_acceptance");
  assert_eq!(worker.executor.calls.load(Ordering::SeqCst), 1);
  assert_eq!(worker.store()?.sessions()?.len(), 1);
  assert_eq!(
    dispatcher
      .accept(&worker.manager.local_node(), &envelope.task_id, true)
      .await?
      .status,
    "completed"
  );
  assert_eq!(
    client
      .get_task(v1::TaskRequest {
        task_id: envelope.task_id
      })
      .await?
      .into_inner()
      .status,
    "completed"
  );
  master_stop.cancel();
  worker_stop.cancel();
  master_server.await??;
  worker_server.await??;
  Ok(())
}

#[tokio::test]
async fn certificate_identity_owner_and_remote_pairing_are_enforced() -> Result<()> {
  let master = Node::new(false).await?;
  let mut worker = Node::new(false).await?;
  let stranger = Node::new(false).await?;
  master.pair(&worker)?;
  worker.pair(&master)?;
  let (stop, server) = worker.serve()?;
  let mut trusted = master.client(&worker).await?;
  let mut spoofed = master.envelope();
  spoofed.owner_node_id = stranger.identity.public.node_id.clone();
  assert_eq!(
    trusted
      .submit_task(spoofed)
      .await
      .expect_err("owner spoof")
      .code(),
    tonic::Code::PermissionDenied
  );
  assert!(worker.store()?.tasks()?.is_empty());
  if let Ok(mut unpaired) = stranger.client(&worker).await {
    assert!(unpaired.submit_task(stranger.envelope()).await.is_err());
  }
  let channel = tonic::transport::Endpoint::from_shared(worker.manager.local_node().endpoint)?
    .tls_config(master.identity.client_tls(&worker.identity.public)?)?
    .connect()
    .await?;
  let mut peers = v1::peer_service_client::PeerServiceClient::new(channel);
  assert_eq!(
    peers
      .pair(v1::PairRequest::default())
      .await
      .expect_err("remote pairing")
      .code(),
    tonic::Code::PermissionDenied
  );
  assert_eq!(worker.store()?.peers()?.len(), 1);
  stop.cancel();
  server.await??;
  Ok(())
}

#[tokio::test]
async fn revocation_denies_existing_tls_channel_stream_and_cancels_cooperative_work() -> Result<()>
{
  let master = Node::new(false).await?;
  let mut worker = Node::new(true).await?;
  master.pair(&worker)?;
  worker.pair(&master)?;
  let (stop, server) = worker.serve()?;
  let mut client = master.client(&worker).await?;
  let envelope = master.envelope();
  client.submit_task(envelope.clone()).await?;
  tokio::time::timeout(Duration::from_secs(5), async {
    while worker.executor.calls.load(Ordering::SeqCst) == 0 {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await?;
  let mut stream = client
    .watch_task(v1::WatchRequest {
      run_id: envelope.task_id.clone(),
      after_cursor: 0,
    })
    .await?
    .into_inner();
  assert!(stream.message().await?.is_some());
  worker
    .manager
    .revoke_local(&master.identity.public.node_id)?;
  assert_eq!(
    client
      .get_task(v1::TaskRequest {
        task_id: envelope.task_id.clone()
      })
      .await
      .expect_err("revoked channel")
      .code(),
    tonic::Code::PermissionDenied
  );
  tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      match stream.message().await {
        Ok(Some(_)) => {}
        Err(status) => {
          assert_eq!(status.code(), tonic::Code::PermissionDenied);
          break;
        }
        Ok(None) => anyhow::bail!("revoked stream ended without denial"),
      }
    }
    loop {
      if worker
        .store()?
        .task(&envelope.task_id)?
        .is_some_and(|task| task.status == "cancelled")
      {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok::<_, anyhow::Error>(())
  })
  .await??;
  stop.cancel();
  drop(stream);
  server.await??;
  Ok(())
}

#[tokio::test]
async fn ca_signed_client_leaf_does_not_inherit_the_paired_certificate_identity() -> Result<()> {
  let master = Node::new(false).await?;
  let mut worker = Node::new(false).await?;
  let mut parameters = rcgen::CertificateParams::new(vec!["synthetic-ca.invalid".into()])?;
  parameters.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
  let ca_key = rcgen::KeyPair::generate()?;
  let certificate = parameters.self_signed(&ca_key)?;
  let mut paired = master.identity.public.clone();
  paired.certificate_pem = certificate.pem();
  worker
    .manager
    .pair_local(paired, vec!["tasks.submit".into(), "test.execute".into()])?;
  let (stop, server) = worker.serve()?;
  let leaf_key = rcgen::KeyPair::generate()?;
  let leaf = rcgen::CertificateParams::new(vec!["different-leaf.invalid".into()])?
    .signed_by(&leaf_key, &rcgen::Issuer::from_params(&parameters, &ca_key))?;
  let channel = tonic::transport::Endpoint::from_shared(worker.manager.local_node().endpoint)?
    .tls_config(
      master
        .identity
        .client_tls(&worker.identity.public)?
        .identity(tonic::transport::Identity::from_pem(
          leaf.pem(),
          leaf_key.serialize_pem(),
        )),
    )?
    .connect()
    .await?;
  let mut client = v1::task_service_client::TaskServiceClient::new(channel);
  let error = client
    .submit_task(master.envelope())
    .await
    .expect_err("only the exact paired leaf is authorized");
  assert_eq!(error.code(), tonic::Code::PermissionDenied);
  assert!(worker.store()?.tasks()?.is_empty());
  stop.cancel();
  server.await??;
  Ok(())
}

#[tokio::test]
async fn master_cancellation_is_confirmed_from_worker_without_resubmission() -> Result<()> {
  let master = Node::new(false).await?;
  let mut worker = Node::new(true).await?;
  master.pair(&worker)?;
  worker.pair(&master)?;
  let (stop, server) = worker.serve()?;
  let dispatcher = Dispatcher::new(master.manager.clone());
  let envelope = master.envelope();
  dispatcher
    .submit(&worker.manager.local_node(), envelope.clone().into())
    .await?;
  tokio::time::timeout(Duration::from_secs(5), async {
    while worker.executor.calls.load(Ordering::SeqCst) == 0 {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await?;
  assert_eq!(
    dispatcher
      .cancel(&worker.manager.local_node(), &envelope.task_id)
      .await?
      .status,
    "cancel_requested"
  );
  tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      let task = dispatcher
        .reconcile(&worker.manager.local_node(), &envelope.task_id)
        .await?;
      if task.status == "cancelled" {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok::<_, anyhow::Error>(())
  })
  .await??;
  assert_eq!(worker.executor.calls.load(Ordering::SeqCst), 1);
  stop.cancel();
  server.await??;
  Ok(())
}

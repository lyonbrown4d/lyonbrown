mod support;
use std::time::Duration;

use agent_distributed::secure_link::{LinkBinding, LinkHandshake};
use anyhow::Result;
use support::{Fixture, frame};

#[tokio::test]
async fn outbound_nodes_exchange_noise_authenticated_ciphertext_without_control_keys() -> Result<()>
{
  let fixture = Fixture::new(8).await?;
  let mut a_client = fixture.client(&fixture.a).await?;
  let mut b_client = fixture.client(&fixture.b).await?;
  let mut a = a_client.open(8).await?;
  let mut b = b_client.open(8).await?;
  let binding = LinkBinding {
    link_id: "test-link".into(),
    initiator: "node-a".into(),
    responder: "node-b".into(),
  };
  let mut initiator = LinkHandshake::new(&fixture.a, &fixture.b.public, binding.clone(), true)?;
  let mut responder = LinkHandshake::new(&fixture.b, &fixture.a.public, binding.clone(), false)?;
  a.try_send(frame("node-a", "node-b", initiator.write()?))?;
  let hello = tokio::time::timeout(Duration::from_secs(3), b.receive())
    .await??
    .expect("hello");
  responder.read(&hello.ciphertext)?;
  b.try_send(frame("node-b", "node-a", responder.write()?))?;
  initiator.read(
    &tokio::time::timeout(Duration::from_secs(3), a.receive())
      .await??
      .expect("reply")
      .ciphertext,
  )?;
  let mut a_link = initiator.finish()?;
  let mut b_link = responder.finish()?;
  let plaintext = b"private task instructions never owned by control";
  let encrypted = a_link.seal(plaintext)?;
  assert!(
    !encrypted
      .ciphertext
      .windows(plaintext.len())
      .any(|part| part == plaintext)
  );
  a.try_send(encrypted.clone())?;
  let delivered = tokio::time::timeout(Duration::from_secs(3), b.receive())
    .await??
    .expect("encrypted frame");
  assert_eq!(delivered, encrypted);
  assert_eq!(b_link.open(&delivered)?, plaintext);
  assert!(
    b_link.open(&delivered).is_err(),
    "E2E endpoint rejects relay replay"
  );
  b.try_send(b_link.seal(b"result")?)?;
  let reply = tokio::time::timeout(Duration::from_secs(3), a.receive())
    .await??
    .expect("encrypted result");
  assert_eq!(a_link.open(&reply)?, b"result");
  let mut wrong_peer = fixture.b.public.clone();
  wrong_peer.noise_public_key = fixture.control.public.noise_public_key.clone();
  let mut wrong = LinkHandshake::new(&fixture.a, &wrong_peer, binding, true)?;
  assert!(
    LinkHandshake::new(
      &fixture.b,
      &fixture.a.public,
      LinkBinding {
        link_id: "test-link".into(),
        initiator: "node-a".into(),
        responder: "node-b".into()
      },
      false
    )?
    .read(&wrong.write()?)
    .is_err(),
    "control TLS trust cannot substitute a paired Noise key"
  );
  assert!(fixture.store.tasks()?.is_empty());
  assert!(fixture.store.sessions()?.is_empty());
  Ok(())
}

#[tokio::test]
async fn active_stream_revocation_and_graph_revocation_deny_further_delivery() -> Result<()> {
  let fixture = Fixture::new(8).await?;
  let mut a_client = fixture.client(&fixture.a).await?;
  let mut b_client = fixture.client(&fixture.b).await?;
  let mut a = a_client.open(8).await?;
  let mut b = b_client.open(8).await?;
  fixture
    .store
    .control_allow_link("node-a", "node-b", false)?;
  a.try_send(frame("node-a", "node-b", vec![7]))?;
  assert_eq!(
    tokio::time::timeout(Duration::from_secs(3), a.receive())
      .await?
      .expect_err("graph revoked")
      .code(),
    tonic::Code::PermissionDenied
  );
  assert!(
    tokio::time::timeout(Duration::from_millis(100), b.receive())
      .await
      .is_err()
  );
  fixture.manager.revoke_local("node-b")?;
  assert_eq!(
    tokio::time::timeout(Duration::from_secs(3), b.receive())
      .await?
      .expect_err("identity revoked")
      .code(),
    tonic::Code::PermissionDenied
  );
  Ok(())
}

#[tokio::test]
async fn disconnected_streams_are_removed_and_reconnect_never_replays_frames() -> Result<()> {
  let fixture = Fixture::new(8).await?;
  let mut a_client = fixture.client(&fixture.a).await?;
  let mut b_client = fixture.client(&fixture.b).await?;
  let a = a_client.open(8).await?;
  let mut b = b_client.open(8).await?;
  assert!(
    a_client.open(8).await.is_err(),
    "one active stream per identity"
  );
  a.try_send(frame("node-a", "node-b", vec![1, 2, 3]))?;
  assert!(
    tokio::time::timeout(Duration::from_secs(3), b.receive())
      .await??
      .is_some()
  );
  drop(b);
  let mut replacement = tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      match b_client.open(8).await {
        Ok(stream) => break Ok::<_, anyhow::Error>(stream),
        Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
      }
    }
  })
  .await??;
  assert!(
    tokio::time::timeout(Duration::from_millis(150), replacement.receive())
      .await
      .is_err(),
    "old ciphertext must not replay after reconnect"
  );
  a.try_send(frame("node-a", "node-b", vec![4]))?;
  assert_eq!(
    tokio::time::timeout(Duration::from_secs(3), replacement.receive())
      .await??
      .expect("fresh frame")
      .ciphertext,
    vec![4]
  );
  fixture.plane.shutdown();
  assert_eq!(
    tokio::time::timeout(Duration::from_secs(3), replacement.receive())
      .await?
      .expect_err("control shutdown closes existing streams")
      .code(),
    tonic::Code::Unavailable
  );
  assert!(
    a_client.open(8).await.is_err(),
    "shutdown refuses new streams"
  );
  Ok(())
}

#[allow(dead_code)]
mod relay_transport_support;
mod support;

use std::time::Duration;

use anyhow::Result;
use relay_transport_support::{RawPeer, Tunnels};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_frame_replay_closes_only_its_link_and_old_nonce_cannot_attach_to_a_new_link()
-> Result<()> {
  let mut raw = RawPeer::start().await?;
  let (mut first, mut secure, first_id) = raw.authenticate().await?;
  let secret = b"private end-to-end bytes";
  let mut payload = vec![1, 0];
  payload.extend_from_slice(secret);
  let frame = secure.seal(&payload)?;
  assert!(
    !frame
      .ciphertext
      .windows(secret.len())
      .any(|part| part == secret)
  );
  raw.remote.try_send(frame.clone())?;
  let mut read = vec![0; secret.len()];
  tokio::time::timeout(Duration::from_secs(3), first.read_exact(&mut read)).await??;
  assert_eq!(read, secret);
  raw.remote.try_send(frame.clone())?;
  assert_eq!(
    tokio::time::timeout(Duration::from_secs(3), first.read(&mut [0])).await??,
    0
  );
  let (mut replacement, mut fresh, fresh_id) = raw.authenticate().await?;
  assert_ne!(first_id, fresh_id);
  raw.remote.try_send(frame)?;
  raw.remote.try_send(fresh.seal(b"\x01\x00fresh")?)?;
  let mut read = [0; 5];
  tokio::time::timeout(Duration::from_secs(3), replacement.read_exact(&mut read)).await??;
  assert_eq!(&read, b"fresh");
  assert!(raw.a.is_connected());
  raw.close().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_authentication_limit_rejects_before_sending_more_handshakes() -> Result<()> {
  let mut raw = RawPeer::start().await?;
  let mut opens = tokio::task::JoinSet::new();
  for _ in 0..8 {
    let handle = raw.a.clone();
    opens.spawn(async move { handle.open("node-b").await });
  }
  for _ in 0..8 {
    let frame = tokio::time::timeout(Duration::from_secs(2), raw.remote.receive())
      .await??
      .expect("handshake");
    assert_eq!(frame.seq, u64::MAX);
  }
  let error = tokio::time::timeout(Duration::from_secs(1), raw.a.open("node-b"))
    .await?
    .err()
    .expect("bounded pending handshakes");
  assert!(error.to_string().contains("limit"));
  opens.abort_all();
  while opens.join_next().await.is_some() {}
  raw.close().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_pair_revocation_or_noise_rekey_closes_active_local_streams() -> Result<()> {
  for rekey in [false, true] {
    let mut tunnels = Tunnels::start().await?;
    let mut left = tunnels.a.open("node-b").await?;
    let mut right = tunnels.incoming_b.next().await.expect("incoming")?;
    left.write_all(b"before").await?;
    right.read_exact(&mut [0; 6]).await?;
    if rekey {
      let mut identity = tunnels.fixture.a.public.clone();
      identity.noise_public_key = "ab".repeat(32);
      tunnels
        .b_manager
        .pair_local(identity, vec!["tasks.read".into()])?;
    } else {
      tunnels.b_manager.revoke_local("node-a")?;
    }
    assert_eq!(
      tokio::time::timeout(Duration::from_secs(2), right.read(&mut [0])).await??,
      0
    );
    assert!(tunnels.a.is_connected() && tunnels.b.is_connected());
    tunnels.close().await?;
  }
  Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnected_configured_route_never_dials_the_direct_endpoint() -> Result<()> {
  let raw = RawPeer::start().await?;
  raw.fixture.plane.shutdown();
  tokio::time::timeout(Duration::from_secs(3), async {
    while raw.a.is_connected() {
      tokio::task::yield_now().await;
    }
  })
  .await?;
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
  let mut remote =
    relay_transport_support::manager(&raw.fixture, &raw.fixture.b, "direct-tripwire.sqlite3")?
      .local_node();
  remote.endpoint = format!("https://{}", listener.local_addr()?);
  let error = raw
    .manager
    .clone()
    .with_relay(raw.a.clone())
    .connect(&remote)
    .await
    .err()
    .expect("disconnected relay is terminal for this connection attempt");
  assert!(format!("{error:#}").contains("no direct-route fallback"));
  assert!(
    tokio::time::timeout(Duration::from_millis(100), listener.accept())
      .await
      .is_err()
  );
  raw.close().await
}

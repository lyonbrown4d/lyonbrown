#[allow(dead_code)]
mod relay_transport_support;
mod support;

use std::time::Duration;

use agent_distributed::{identity::PeerIdentity, relay_transport::PeerIo};
use agent_protocol::v1::peer_service_server::PeerServiceServer;
use anyhow::Result;
use relay_transport_support::{Tunnels, manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_authenticated_tunnels_preserve_bytes_and_half_close_without_extra_relay_streams()
-> Result<()> {
  let mut tunnels = Tunnels::start().await?;
  let mut channels = Vec::new();
  for index in 0..4 {
    let left = tunnels.a.open("node-b").await?;
    let right = tokio::time::timeout(Duration::from_secs(3), tunnels.incoming_b.next())
      .await?
      .expect("incoming")?;
    channels.push((index, left, right));
  }
  let mut tasks = tokio::task::JoinSet::new();
  for (index, mut left, mut right) in channels {
    tasks.spawn(async move {
      let bytes = format!("private framed bytes for tunnel {index}");
      left.write_all(bytes.as_bytes()).await?;
      left.shutdown().await?;
      let mut read = Vec::new();
      right.read_to_end(&mut read).await?;
      assert_eq!(read, bytes.as_bytes());
      right.write_all(b"worker response after EOF").await?;
      right.shutdown().await?;
      let mut response = Vec::new();
      left.read_to_end(&mut response).await?;
      assert_eq!(response, b"worker response after EOF");
      Ok::<_, anyhow::Error>(())
    });
  }
  tokio::time::timeout(Duration::from_secs(5), async {
    while let Some(task) = tasks.join_next().await {
      task??;
    }
    Ok::<_, anyhow::Error>(())
  })
  .await??;
  assert!(
    tunnels
      .fixture
      .client(&tunnels.fixture.a)
      .await?
      .open(8)
      .await
      .is_err()
  );
  assert!(tunnels.fixture.store.tasks()?.is_empty());
  assert!(tunnels.fixture.store.sessions()?.is_empty());
  tunnels.close().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inner_mutual_tls_uses_existing_peer_authorization_and_rejects_an_unpaired_certificate()
-> Result<()> {
  let mut tunnels = Tunnels::start().await?;
  let (_, empty) = tokio::sync::mpsc::channel(1);
  let incoming = std::mem::replace(
    &mut tunnels.incoming_b,
    tokio_stream::wrappers::ReceiverStream::new(empty),
  );
  let service = tunnels.b_manager.clone();
  let tls = service.server_tls()?;
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .tls_config(tls)?
      .add_service(PeerServiceServer::new(service))
      .serve_with_incoming(incoming.map(|io| io.map(PeerIo::Relay)))
      .await
  });
  let remote = tunnels.b_manager.local_node();
  let paired = tunnels.a_manager.clone().with_relay(tunnels.a.clone());
  assert_eq!(paired.connect(&remote).await?.node.id, "node-b");
  let stranger = PeerIdentity::generate("stranger")?;
  let stranger_manager = manager(&tunnels.fixture, &stranger, "stranger.sqlite3")?;
  stranger_manager.pair_local(tunnels.fixture.b.public.clone(), Vec::new())?;
  // Noise still authenticates node-a. The independent inner TLS certificate must also pass.
  assert!(
    stranger_manager
      .with_relay(tunnels.a.clone())
      .connect(&remote)
      .await
      .is_err()
  );
  assert_eq!(paired.connect(&remote).await?.node.id, "node-b");
  server.abort();
  let _ = server.await;
  tunnels.close().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn link_limit_and_slow_reader_backpressure_are_bounded_and_other_links_continue() -> Result<()>
{
  let mut tunnels = Tunnels::start().await?;
  let mut channels = Vec::new();
  for _ in 0..32 {
    let left = tunnels.a.open("node-b").await?;
    let right = tunnels.incoming_b.next().await.expect("incoming")?;
    channels.push((left, right));
  }
  let error = tunnels
    .a
    .open("node-b")
    .await
    .err()
    .expect("bounded link count");
  assert!(error.to_string().contains("limit"));
  let (mut slow_left, mut slow_right) = channels.remove(0);
  let (mut good_left, mut good_right) = channels.remove(0);
  let mut flood = tokio::spawn(async move {
    slow_left.write_all(&vec![42; 1024 * 1024]).await?;
    Ok::<_, std::io::Error>(slow_left)
  });
  assert!(
    tokio::time::timeout(Duration::from_millis(100), &mut flood)
      .await
      .is_err(),
    "a slow reader must apply bounded backpressure instead of buffering the whole payload"
  );
  good_left.write_all(b"healthy").await?;
  let mut read = [0; 7];
  tokio::time::timeout(Duration::from_secs(3), good_right.read_exact(&mut read)).await??;
  assert_eq!(&read, b"healthy");
  assert!(tunnels.a.is_connected() && tunnels.b.is_connected());
  let reader = tokio::spawn(async move {
    let mut bytes = vec![0; 1024 * 1024];
    slow_right.read_exact(&mut bytes).await?;
    Ok::<_, std::io::Error>(bytes)
  });
  let _sender = tokio::time::timeout(Duration::from_secs(5), flood).await???;
  let bytes = tokio::time::timeout(Duration::from_secs(5), reader).await???;
  assert!(bytes.iter().all(|byte| *byte == 42));
  drop(channels);
  tunnels.close().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_admitted_links_transfer_simultaneous_bidirectional_bursts_without_disconnect()
-> Result<()> {
  for queue in [1, 64] {
    burst(queue).await?;
  }
  Ok(())
}

async fn burst(queue: usize) -> Result<()> {
  let mut tunnels = Tunnels::with_queue(queue).await?;
  let mut tasks = tokio::task::JoinSet::new();
  let mut channels = Vec::new();
  for index in 1_u8..=32 {
    let left = tunnels.a.open("node-b").await?;
    let right = tunnels.incoming_b.next().await.expect("incoming")?;
    channels.push((index, left, right));
  }
  for (index, left, right) in channels {
    tasks.spawn(async move {
      let (mut left_read, mut left_write) = tokio::io::split(left);
      let (mut right_read, mut right_write) = tokio::io::split(right);
      let outgoing = vec![index; 128 * 1024];
      let returning = vec![index + 128; 128 * 1024];
      let mut arrived = vec![0; outgoing.len()];
      let mut returned = vec![0; returning.len()];
      tokio::try_join!(
        left_write.write_all(&outgoing),
        right_write.write_all(&returning),
        right_read.read_exact(&mut arrived),
        left_read.read_exact(&mut returned)
      )?;
      assert_eq!(arrived, outgoing);
      assert_eq!(returned, returning);
      Ok::<_, anyhow::Error>(())
    });
  }
  tokio::time::timeout(Duration::from_secs(15), async {
    while let Some(task) = tasks.join_next().await {
      task??;
    }
    Ok::<_, anyhow::Error>(())
  })
  .await??;
  assert!(tunnels.a.is_connected() && tunnels.b.is_connected());
  tunnels.close().await
}

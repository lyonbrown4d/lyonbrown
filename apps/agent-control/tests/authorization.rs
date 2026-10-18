mod support;
use std::time::Duration;

use agent_distributed::identity::PeerIdentity;
use agent_protocol::v1;
use agent_storage::Store;
use anyhow::Result;
use support::{Fixture, frame, node};
use tokio_stream::wrappers::ReceiverStream;

#[tokio::test]
async fn directory_binds_tls_and_noise_identity_and_persists_only_operator_authorized_metadata()
-> Result<()> {
  let fixture = Fixture::new(8).await?;
  let mut a = fixture.client(&fixture.a).await?;
  let mut b = fixture.client(&fixture.b).await?;
  let registered = b.register(node(&fixture.b)).await?;
  assert_eq!(a.resolve("node-b").await?, registered);
  let reopened = Store::open(fixture.directory.path().join("control.sqlite3"))?;
  assert!(reopened.control_link_allowed("node-a", "node-b")?);
  assert!(
    reopened
      .control_resolve("node-b", agent_core::now_ms())?
      .is_some()
  );
  let mut raw = fixture.raw(&fixture.a).await?;
  let spoof = v1::RegisterRequest {
    node: Some(node(&fixture.b)),
    public_key: fixture.b.public.noise_public_key.clone(),
  };
  assert_eq!(
    raw
      .register(spoof)
      .await
      .expect_err("register as another node")
      .code(),
    tonic::Code::PermissionDenied
  );
  assert!(
    reopened
      .control_resolve("node-a", agent_core::now_ms())?
      .is_none()
  );
  let bad_noise = v1::RegisterRequest {
    node: Some(node(&fixture.a)),
    public_key: fixture.b.public.noise_public_key.clone(),
  };
  assert_eq!(
    raw
      .register(bad_noise)
      .await
      .expect_err("replace paired Noise identity")
      .code(),
    tonic::Code::PermissionDenied
  );
  assert_eq!(
    raw
      .pair(v1::PairRequest::default())
      .await
      .expect_err("remote pairing")
      .code(),
    tonic::Code::PermissionDenied
  );
  assert_eq!(fixture.store.peers()?.len(), 2);
  fixture
    .store
    .control_allow_link("node-a", "node-b", false)?;
  assert!(
    fixture.store.control_link_allowed("node-b", "node-a")?,
    "operator policy is directional"
  );
  assert_eq!(
    raw
      .resolve(v1::PeerRequest {
        node_id: "node-b".into()
      })
      .await
      .expect_err("operator scope")
      .code(),
    tonic::Code::PermissionDenied
  );
  fixture.store.control_allow_link("node-a", "node-b", true)?;
  tokio::time::sleep(Duration::from_millis(1100)).await;
  assert_eq!(
    raw
      .resolve(v1::PeerRequest {
        node_id: "node-b".into()
      })
      .await
      .expect_err("expired lease")
      .code(),
    tonic::Code::NotFound
  );
  b.register(registered.clone()).await?;
  assert_eq!(a.resolve("node-b").await?, registered);
  Ok(())
}

#[tokio::test]
async fn unpaired_certificates_cannot_register_or_open_relay() -> Result<()> {
  let fixture = Fixture::new(8).await?;
  let stranger = PeerIdentity::generate("stranger")?;
  if let Ok(mut raw) = fixture.raw(&stranger).await {
    assert!(
      raw
        .register(v1::RegisterRequest {
          node: Some(node(&stranger)),
          public_key: stranger.public.noise_public_key.clone()
        })
        .await
        .is_err()
    );
    let (_sender, receiver) = tokio::sync::mpsc::channel::<v1::RelayEnvelope>(1);
    assert!(raw.relay(ReceiverStream::new(receiver)).await.is_err());
  }
  assert!(
    fixture
      .store
      .control_resolve("stranger", agent_core::now_ms())?
      .is_none()
  );
  assert_eq!(fixture.store.peers()?.len(), 2);
  Ok(())
}

#[tokio::test]
async fn server_rejects_source_spoofing_unauthorized_targets_and_invalid_frames() -> Result<()> {
  let fixture = Fixture::new(8).await?;
  let mut b = fixture.client(&fixture.b).await?.open(8).await?;
  let cases = [
    (
      frame("node-b", "node-b", vec![1]),
      tonic::Code::PermissionDenied,
    ),
    (
      frame("node-a", "control", vec![1]),
      tonic::Code::PermissionDenied,
    ),
    (
      frame("node-a", "node-b", vec![1; 65_536]),
      tonic::Code::InvalidArgument,
    ),
    (
      frame("node-a", "node-b", vec![]),
      tonic::Code::InvalidArgument,
    ),
    (
      v1::RelayEnvelope {
        link_id: String::new(),
        ..frame("node-a", "node-b", vec![1])
      },
      tonic::Code::InvalidArgument,
    ),
  ];
  for (frame, code) in cases {
    let mut client = fixture.raw(&fixture.a).await?;
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let mut output = client
      .relay(ReceiverStream::new(receiver))
      .await?
      .into_inner();
    sender.send(frame).await?;
    assert_eq!(
      tokio::time::timeout(Duration::from_secs(3), output.message())
        .await?
        .expect_err("rejected frame")
        .code(),
      code
    );
  }
  assert!(
    tokio::time::timeout(Duration::from_millis(100), b.receive())
      .await
      .is_err(),
    "rejected frames must not reach target"
  );
  Ok(())
}

#[tokio::test]
async fn bounded_relay_reports_backpressure_to_sender_instead_of_silent_loss() -> Result<()> {
  let fixture = Fixture::new(1).await?;
  let _slow = fixture.client(&fixture.b).await?.open(1).await?;
  let mut raw = fixture.raw(&fixture.a).await?;
  let (sender, receiver) = tokio::sync::mpsc::channel(1);
  let mut output = raw.relay(ReceiverStream::new(receiver)).await?.into_inner();
  let writer = tokio::spawn(async move {
    for seq in 0..2048 {
      if sender
        .send(v1::RelayEnvelope {
          seq,
          ..frame("node-a", "node-b", vec![7; 65_535])
        })
        .await
        .is_err()
      {
        break;
      }
    }
  });
  let failure = tokio::time::timeout(Duration::from_secs(5), output.message())
    .await?
    .expect_err("bounded destination queue");
  writer.abort();
  assert_eq!(failure.code(), tonic::Code::ResourceExhausted);
  Ok(())
}

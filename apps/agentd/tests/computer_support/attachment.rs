use super::{RawHost, desktop, detached, hello, reject_unauthenticated};
use crate::node;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Code;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_hello_binds_immutable_scope_and_rejects_forgery_or_duplicates() {
  let (node, _mock) = node().await;
  reject_unauthenticated(&node).await;
  let desktop = desktop(&node);
  let host = RawHost::attach(&node, &desktop).await;
  let info = node
    .daemon
    .computer_runtime_info(&node.session_id)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(info.host_id, host.host);
  assert_eq!(info.daemon_session_id, node.session_id);
  assert_eq!(info.owner_epoch, 1);
  assert_eq!(info.desktop_session, desktop);
  let foreign = node
    .daemon
    .store
    .create_session(
      "foreign",
      node.workspace.path().to_str().unwrap(),
      "other-node",
    )
    .unwrap();
  let owned = node
    .client
    .create_session("other", node.workspace.path().to_str().unwrap())
    .await
    .unwrap();
  let mut forged = desktop.clone();
  forged.identity.node_id = "forged-node".into();
  let cases = [
    (
      hello(&Uuid::new_v4().to_string(), &owned.id, &forged),
      Code::PermissionDenied,
    ),
    (
      hello(
        &Uuid::new_v4().to_string(),
        &Uuid::new_v4().to_string(),
        &desktop,
      ),
      Code::NotFound,
    ),
    (
      hello(&Uuid::new_v4().to_string(), &foreign.id, &desktop),
      Code::PermissionDenied,
    ),
    (
      hello(&Uuid::new_v4().to_string(), &node.session_id, &desktop),
      Code::ResourceExhausted,
    ),
    (
      hello(&host.host, &owned.id, &desktop),
      Code::ResourceExhausted,
    ),
  ];
  for (packet, expected) in cases {
    let (sender, receiver) = mpsc::channel(1);
    sender.send(packet).await.unwrap();
    let error = match node
      .client
      .attach_computer_runtime(ReceiverStream::new(receiver))
      .await
    {
      Err(error) => error,
      Ok(_) => panic!("hello must be rejected"),
    };
    assert_eq!(
      error.downcast_ref::<tonic::Status>().unwrap().code(),
      expected
    );
  }
  assert_eq!(
    node
      .daemon
      .computer_runtime_info(&node.session_id)
      .await
      .unwrap()
      .unwrap()
      .desktop_session,
    desktop
  );
  drop(host);
  detached(&node).await;
  node.close().await;
}

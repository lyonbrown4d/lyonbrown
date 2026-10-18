#[path = "support/attachment_backend.rs"]
mod backend;
#[path = "support/attachment.rs"]
mod transport;

use agent_computer_runtime::*;
use backend::Host;
use std::{sync::atomic::Ordering, time::Duration};
use transport::Server;
use uuid::Uuid;

async fn stopped(attachment: &RuntimeAttachment) {
  tokio::time::timeout(Duration::from_secs(2), async {
    while !attachment.is_finished() {
      tokio::task::yield_now().await;
    }
  })
  .await
  .expect("attachment stop deadline");
}

#[tokio::test]
async fn authenticated_hello_exact_binding_and_uuid_replay_exclusion() {
  let mut server = Server::start().await;
  let host = Host::new(&server.session.owner_node_id, &server.session.id);
  host.gate.release();
  let attachment = attach_host(
    server.client.clone(),
    &server.session.id,
    host.runtime.clone(),
  )
  .await
  .expect("attach");
  let hello = server.response().await;
  assert!(hello.request_id.is_empty());
  assert_eq!(hello.host_id, attachment.host_id);
  assert_eq!(hello.daemon_session_id, server.session.id);
  assert_eq!(hello.protocol_version, RUNTIME_PROTOCOL_VERSION);
  let hello: RuntimeHello = wire::decode(&hello.payload).expect("hello payload");
  assert_eq!(&hello.desktop_session, host.runtime.session());
  let id = Uuid::new_v4();
  server.send(host.request(id, host.execute())).await;
  let response = server.response().await;
  assert_eq!(response.request_id, id.to_string());
  assert_eq!(response.host_id, attachment.host_id);
  assert_eq!(response.daemon_session_id, server.session.id);
  let result: RuntimeResult<Reply> = wire::decode(&response.payload).expect("result packet");
  assert!(matches!(result, Ok(Reply::Executed(_))));
  assert_eq!(host.gate.input_count(), 1);
  let mut duplicate = host.request(id, host.execute());
  duplicate.request_id = id.simple().to_string();
  server.send(duplicate).await;
  stopped(&attachment).await;
  assert!(
    attachment
      .error()
      .expect("replay error")
      .contains("duplicate")
  );
  assert_eq!(host.gate.input_count(), 1);
}

#[tokio::test]
async fn attachment_checks_selected_session_and_authenticated_node_before_hello() {
  let mut server = Server::start().await;
  let host = Host::new(&server.session.owner_node_id, &server.session.id);
  assert!(
    attach_host(
      server.client.clone(),
      &Uuid::new_v4().to_string(),
      host.runtime.clone()
    )
    .await
    .is_err()
  );
  let other = Host::new("wrong-node", &server.session.id);
  assert!(
    attach_host(server.client.clone(), &server.session.id, other.runtime)
      .await
      .is_err()
  );
  let mut attachment = attach_host(server.client.clone(), &server.session.id, host.runtime)
    .await
    .expect("valid attachment");
  let _hello = server.response().await;
  {
    use std::{
      future::{Future, poll_fn},
      task::Poll,
    };
    let mut waiting = Box::pin(attachment.wait_closed());
    assert!(poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx).is_pending())).await);
    // Cancelling a wait future must retain its task for explicit shutdown.
  }
  attachment.shutdown().await.expect("shutdown");
  attachment.wait_closed().await.expect("already closed");
  attachment.shutdown().await.expect("idempotent shutdown");
  assert!(attachment.is_finished());
  assert!(attachment.error().is_none());
  assert_eq!(host.gate.input_count(), 0);
}

#[tokio::test]
async fn malformed_or_cross_session_commands_cancel_without_input() {
  for kind in ["payload", "binding", "identifier"] {
    let mut server = Server::start().await;
    let mut host = Host::new(&server.session.owner_node_id, &server.session.id);
    let attachment = attach_host(
      server.client.clone(),
      &server.session.id,
      host.runtime.clone(),
    )
    .await
    .expect("attach");
    let _hello = server.response().await;
    if kind == "binding" {
      host.binding.daemon_session_id = Uuid::new_v4().to_string();
    }
    let mut request = host.request(Uuid::new_v4(), host.execute());
    if kind == "payload" {
      request.payload = vec![255];
    }
    if kind == "identifier" {
      request.request_id = "not-a-uuid".into();
    }
    server.send(request).await;
    stopped(&attachment).await;
    assert!(attachment.error().is_some(), "{kind} rejected");
    assert_eq!(host.gate.input_count(), 0);
  }
}

#[tokio::test]
async fn overlap_stream_failure_and_drop_cancel_blocking_input_without_replay() {
  for kind in ["overlap", "eof", "error", "drop"] {
    let mut server = Server::start().await;
    let host = Host::new(&server.session.owner_node_id, &server.session.id);
    let attachment = attach_host(
      server.client.clone(),
      &server.session.id,
      host.runtime.clone(),
    )
    .await
    .expect("attach");
    let _hello = server.response().await;
    server
      .send(host.request(Uuid::new_v4(), host.execute()))
      .await;
    tokio::time::timeout(Duration::from_secs(2), host.gate.entered.notified())
      .await
      .expect("blocking worker started");
    assert_eq!(host.gate.input_count(), 0);
    if kind == "drop" {
      drop(attachment);
    } else {
      if kind == "overlap" {
        server
          .send(host.request(Uuid::new_v4(), host.execute()))
          .await;
      } else if kind == "error" {
        server
          .requests
          .as_ref()
          .expect("request stream")
          .send(Err(tonic::Status::unavailable("fixture disconnect")))
          .await
          .expect("stream error");
      } else {
        drop(server.requests.take());
      }
      stopped(&attachment).await;
      assert!(attachment.error().is_some());
      drop(attachment);
    }
    host.gate.release();
    tokio::time::timeout(Duration::from_secs(2), host.gate.finished.notified())
      .await
      .expect("native worker drained");
    assert!(
      host.gate.cancelled.load(Ordering::SeqCst),
      "{kind} invalidated native input"
    );
    assert_eq!(host.gate.input_count(), 0, "{kind} produced no input");
  }
}

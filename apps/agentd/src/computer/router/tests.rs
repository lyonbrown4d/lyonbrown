use super::*;
use agent_computer::{DesktopIdentity, DesktopSession, PermissionState, TargetScope};
use agent_computer_runtime::{RUNTIME_PROTOCOL_VERSION, wire};
use std::{future::Future, future::poll_fn, task::Poll};
use tonic::Code;

#[tokio::test]
async fn cancelled_attachment_can_be_replaced_before_reader_cleanup_without_adopting_old_work() {
  let router = Arc::new(ComputerRouter::default());
  let info = ComputerRuntimeInfo {
    host_id: Uuid::new_v4().to_string(),
    daemon_session_id: Uuid::new_v4().to_string(),
    owner_epoch: 1,
    desktop_session: DesktopSession {
      id: Uuid::new_v4(),
      identity: DesktopIdentity {
        node_id: Uuid::new_v4().to_string(),
        user_session: "synthetic-user".into(),
      },
      scope: TargetScope::Window(7),
    },
  };
  let binding = RunBinding {
    daemon_session_id: info.daemon_session_id.clone(),
    run_id: Uuid::new_v4().to_string(),
    owner_epoch: 1,
  };
  let (outgoing, mut old_requests) = mpsc::channel(1);
  let stop = CancellationToken::new();
  let old_generation = router
    .register(info.clone(), outgoing, stop.clone())
    .await
    .unwrap();
  let old_pin = router.pin(&info.daemon_session_id, 1).await.unwrap();
  let mut old_request = Box::pin(router.request(
    &old_pin,
    binding.clone(),
    Operation::Capabilities,
    CancellationToken::new(),
  ));
  assert!(poll_fn(|cx| Poll::Ready(old_request.as_mut().poll(cx).is_pending())).await);
  old_requests.try_recv().unwrap().unwrap();

  // Model response-stream drop before its reader task reaches disconnect().
  // No reader is scheduled here, so the replacement window is deterministic.
  stop.cancel();
  assert!(router.info(&info.daemon_session_id, 1).await.is_none());
  let (outgoing, mut requests) = mpsc::channel(1);
  let generation = router
    .register(info.clone(), outgoing, CancellationToken::new())
    .await
    .expect("a cancelled attachment must release its registration immediately");
  assert_ne!(old_generation, generation);
  assert_eq!(old_request.await.err().unwrap().code(), Code::Unavailable);
  assert_eq!(
    router
      .request(
        &old_pin,
        binding.clone(),
        Operation::Capabilities,
        CancellationToken::new(),
      )
      .await
      .err()
      .unwrap()
      .code(),
    Code::Unavailable,
    "a queued old command must not dispatch to the replacement"
  );
  let (duplicate, _) = mpsc::channel(1);
  assert_eq!(
    router
      .register(info.clone(), duplicate, CancellationToken::new())
      .await
      .unwrap_err()
      .code(),
    Code::ResourceExhausted,
    "a live attachment must still exclude another host"
  );

  let pin = router.pin(&info.daemon_session_id, 1).await.unwrap();
  let mut request = Box::pin(router.request(
    &pin,
    binding,
    Operation::Capabilities,
    CancellationToken::new(),
  ));
  assert!(poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx).is_pending())).await);
  let dispatched = requests.try_recv().unwrap().unwrap();
  let capabilities = DesktopCapabilities {
    identity: info.desktop_session.identity.clone(),
    interactive: true,
    capture: PermissionState::Granted,
    input: PermissionState::Granted,
    accessibility: PermissionState::Unsupported,
  };
  let reply: RuntimeResult<Reply> = Ok(Reply::Capabilities(capabilities.clone()));
  let response = v1::ComputerRuntimeResponse {
    request_id: dispatched.request_id,
    host_id: info.host_id.clone(),
    daemon_session_id: info.daemon_session_id.clone(),
    protocol_version: RUNTIME_PROTOCOL_VERSION,
    payload: wire::encode(&reply).unwrap(),
  };
  assert!(
    router
      .receive(&info.daemon_session_id, old_generation, response.clone())
      .await
      .is_err(),
    "a late old response must not fulfill the replacement request"
  );
  router
    .disconnect(&info.daemon_session_id, old_generation)
    .await;
  assert!(router.info(&info.daemon_session_id, 1).await.is_some());
  assert!(poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx).is_pending())).await);
  router
    .receive(&info.daemon_session_id, generation, response)
    .await
    .unwrap();
  let result = request.await.unwrap().unwrap();
  let Reply::Capabilities(result) = result else {
    panic!("replacement must return capabilities")
  };
  assert_eq!(result, capabilities);
  assert!(
    requests.try_recv().is_err(),
    "old work must not be replayed"
  );
  router.disconnect(&info.daemon_session_id, generation).await;
}

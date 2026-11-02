use crate::test_support as support;

use crate::{
  computer,
  connection::Update,
  tabs::{ScopedUpdate, TabSender},
  workbench::state::Reply,
};
use gpui_kit::TestAppContext;
use std::{future::poll_fn, task::Poll};

#[gpui_kit::test]
async fn burst_completion_and_pending_operation_waits_wake_without_advancing_clock(
  cx: &mut TestAppContext,
) {
  let harness = support::Harness::new(cx, Vec::new());
  let sender = cx.update(|cx| {
    harness.view.update(cx, |view, _| {
      view.navigation.pending_requests = 1;
      for index in 0..256 {
        view
          .sender
          .try_send(ScopedUpdate::Tab {
            id: view.navigation.active,
            update: Update::Error(format!("completion {index}")),
          })
          .expect("bounded burst");
      }
      view.sender.clone()
    })
  });
  // Drive the actual Tokio producer on the test thread. The deterministic
  // GPUI scheduler must receive its wake only from that supported boundary.
  let final_reply = harness
    .client_runtime
    .spawn(async move { sender.send(ScopedUpdate::RequestFinished).await });
  harness.client_runtime.block_on(tokio::task::yield_now());
  assert!(
    !final_reply.is_finished(),
    "full queue must retain the final reply"
  );
  cx.run_until_parked();
  harness
    .client_runtime
    .block_on(final_reply)
    .expect("Tokio producer")
    .expect("final acknowledgement");
  cx.run_until_parked();
  cx.update(|cx| {
    let view = harness.view.read(cx);
    assert_eq!(view.vm.error.as_deref(), Some("completion 255"));
    assert_eq!(view.navigation.pending_requests, 0);
  });

  let workbench = cx.update(|cx| harness.view.read(cx).tasks.clone());
  for (reason, reply) in [
    (
      crate::block::BlockReason::SavePending,
      Reply::SaveError("save conflict".into()),
    ),
    (
      crate::block::BlockReason::BrowserPending,
      Reply::BrowserOperationError("browser unavailable".into()),
    ),
  ] {
    cx.update(|cx| {
      harness
        .view
        .update(cx, |view, _| view.blocked = Some(reason));
      workbench.update(cx, |view, _| {
        view.pending_save = reason == crate::block::BlockReason::SavePending;
        view.pending_browser_operation = reason == crate::block::BlockReason::BrowserPending;
        view
          .sender
          .try_send((view.generation, reply))
          .expect("operation completion");
      });
    });
    cx.run_until_parked();
    cx.update(|cx| {
      let view = harness.view.read(cx);
      assert!(
        view.blocked.is_none(),
        "completed work clears the recorded wait reason"
      );
      // The earlier transport error stays in `vm.error`: wait text is derived
      // from `blocked`, so completion must not clear a real error either.
      assert_eq!(view.vm.error.as_deref(), Some("completion 255"));
      assert!(!workbench.read(cx).pending_save);
      assert!(!workbench.read(cx).pending_browser_operation);
    });
  }

  cx.update(|cx| {
    harness.view.update(cx, |view, _| {
      view.blocked = Some(crate::block::BlockReason::ComputerPending);
    });
    workbench.read(cx).computer.clone().update(cx, |view, _| {
      view.pending = true;
      view
        .sender
        .try_send((view.generation, computer::Reply::Error("host unavailable")))
        .expect("computer completion");
    });
  });
  cx.run_until_parked();
  cx.update(|cx| {
    assert!(harness.view.read(cx).blocked.is_none());
    assert_eq!(
      harness.view.read(cx).vm.error.as_deref(),
      Some("completion 255")
    );
    assert!(!workbench.read(cx).computer.read(cx).pending);
  });
}

#[tokio::test]
async fn receiver_drop_aborts_buffered_watch_and_releases_waiting_final_reply() {
  let (sender, receiver) = crate::completion::channel(1);
  let tabs = TabSender {
    id: 1,
    sender: sender.clone(),
  };
  let watch = tokio::spawn(std::future::pending::<()>());
  let abort = watch.abort_handle();
  tabs.attach_watch(watch).await;
  let mut final_reply = Box::pin(sender.send(ScopedUpdate::RequestFinished));
  assert!(
    poll_fn(|cx| Poll::Ready(final_reply.as_mut().poll(cx)))
      .await
      .is_pending()
  );
  drop(receiver);
  assert!(final_reply.await.is_err());
  tokio::task::yield_now().await;
  assert!(
    abort.is_finished(),
    "buffered local watch cannot retain a sender cycle"
  );
}

#[tokio::test]
async fn cancelled_unpolled_watch_registration_aborts_local_subscription() {
  let (sender, _receiver) = crate::completion::channel(1);
  let tabs = TabSender { id: 1, sender };
  let watch = tokio::spawn(std::future::pending::<()>());
  let abort = watch.abort_handle();
  let registration = tabs.attach_watch(watch);
  drop(registration);
  tokio::task::yield_now().await;
  assert!(abort.is_finished());
}

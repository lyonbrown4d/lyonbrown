use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol, text_reply};
use gpui_kit::{AppContext as _, TestAppContext, test::TestWindowExt as _};

#[path = "../../agentd/tests/peer_support/mod.rs"]
#[allow(dead_code)]
mod peer_support;
#[path = "../../agentd/tests/remote_support/mod.rs"]
#[allow(dead_code)]
mod remote_support;

use remote_support::Remote;
#[path = "support/tasks.rs"]
mod task_support;
use task_support::{TaskUi, task_ids, worker_target};

#[gpui_kit::test]
async fn task_controls_dispatch_accept_reject_and_cancel_with_durable_master_and_worker_state(
  cx: &mut TestAppContext,
) {
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(2)
    .enable_all()
    .build()
    .expect("remote fixture runtime");
  let mock = runtime
    .block_on(MockModelServer::start(vec![
      text_reply(StandardProtocol::ChatCompletions, "accepted worker output"),
      text_reply(StandardProtocol::ChatCompletions, "rejected worker output"),
      ScriptedReply::stall(),
    ]))
    .expect("synthetic model server");
  let remote = runtime.block_on(Remote::open(&mock, StandardProtocol::ChatCompletions));
  let _parent = remote.pair.envelope(vec!["workspace.read".into()]);
  let target = worker_target(&remote);
  let ui = TaskUi::new(cx, &remote.endpoint, remote.pair.master.auth_token());
  ui.connect(cx);
  ui.wait_connected(cx).await;
  ui.select_session_and_open_tasks(cx);
  ui.wait_active_run(cx).await;
  ui.set_target(cx, &target.id, &target.endpoint);

  ui.click(cx, "task-dispatch");
  ui.wait(cx, || {
    remote.pair.master.store.tasks().is_ok_and(|tasks| {
      tasks
        .iter()
        .filter(|task| task.envelope.session_id == remote.pair.session.id)
        .count()
        == 1
    })
  })
  .await;
  let first = task_ids(&remote).await.remove(0);
  ui.wait(cx, || {
    remote
      .pair
      .worker
      .store
      .task(&first)
      .ok()
      .flatten()
      .is_some_and(|task| task.attempt_status == "succeeded")
  })
  .await;
  ui.click(cx, "task-refresh");
  ui.wait_element(
    cx,
    gpui_kit::SharedString::from(format!("task-reconcile-{first}")),
  )
  .await;
  ui.click(
    cx,
    gpui_kit::SharedString::from(format!("task-reconcile-{first}")),
  );
  ui.wait(cx, || {
    remote
      .pair
      .master
      .store
      .task(&first)
      .ok()
      .flatten()
      .is_some_and(|task| task.status == "awaiting_acceptance")
  })
  .await;
  ui.click(
    cx,
    gpui_kit::SharedString::from(format!("task-accept-{first}")),
  );
  ui.wait(cx, || {
    remote
      .pair
      .master
      .store
      .task(&first)
      .ok()
      .flatten()
      .is_some_and(|task| task.status == "completed")
  })
  .await;
  assert_eq!(
    remote
      .pair
      .worker
      .store
      .task(&first)
      .expect("worker read")
      .expect("worker task")
      .status,
    "completed"
  );

  ui.click(cx, "task-dispatch");
  ui.wait(cx, || {
    remote.pair.master.store.tasks().is_ok_and(|tasks| {
      tasks
        .iter()
        .filter(|task| task.envelope.session_id == remote.pair.session.id)
        .count()
        == 2
    })
  })
  .await;
  let ids = task_ids(&remote).await;
  let second = ids
    .into_iter()
    .find(|id| id != &first)
    .expect("second stable task ID");
  ui.wait(cx, || {
    remote
      .pair
      .worker
      .store
      .task(&second)
      .ok()
      .flatten()
      .is_some_and(|task| task.attempt_status == "succeeded")
  })
  .await;
  ui.click(cx, "task-refresh");
  ui.wait_element(
    cx,
    gpui_kit::SharedString::from(format!("task-reconcile-{second}")),
  )
  .await;
  ui.click(
    cx,
    gpui_kit::SharedString::from(format!("task-reconcile-{second}")),
  );
  ui.wait(cx, || {
    remote
      .pair
      .master
      .store
      .task(&second)
      .ok()
      .flatten()
      .is_some_and(|task| task.status == "awaiting_acceptance")
  })
  .await;
  ui.click(cx, "task-refresh");
  ui.wait_element(
    cx,
    gpui_kit::SharedString::from(format!("task-reject-{second}")),
  )
  .await;
  ui.click(
    cx,
    gpui_kit::SharedString::from(format!("task-reject-{second}")),
  );
  ui.wait(cx, || {
    remote
      .pair
      .master
      .store
      .task(&second)
      .ok()
      .flatten()
      .is_some_and(|task| task.status == "rejected")
  })
  .await;
  assert_eq!(
    remote
      .pair
      .worker
      .store
      .task(&second)
      .expect("worker read")
      .expect("worker task")
      .status,
    "rejected"
  );

  ui.click(cx, "task-dispatch");
  ui.wait(cx, || {
    remote.pair.master.store.tasks().is_ok_and(|tasks| {
      tasks
        .iter()
        .filter(|task| task.envelope.session_id == remote.pair.session.id)
        .count()
        == 3
    })
  })
  .await;
  let ids = task_ids(&remote).await;
  let third = ids
    .into_iter()
    .find(|id| id != &first && id != &second)
    .expect("third stable task ID");
  ui.wait(cx, || {
    remote
      .pair
      .worker
      .store
      .task(&third)
      .ok()
      .flatten()
      .is_some_and(|task| task.attempt_status == "running")
      && runtime.block_on(mock.requests()).len() == 3
  })
  .await;
  ui.click(cx, "task-refresh");
  ui.wait_element(
    cx,
    gpui_kit::SharedString::from(format!("task-cancel-{third}")),
  )
  .await;
  ui.click(
    cx,
    gpui_kit::SharedString::from(format!("task-cancel-{third}")),
  );
  ui.wait(cx, || {
    remote
      .pair
      .worker
      .store
      .task(&third)
      .ok()
      .flatten()
      .is_some_and(|task| task.status == "cancelled")
  })
  .await;
  assert_eq!(
    remote
      .pair
      .master
      .store
      .task(&third)
      .unwrap()
      .unwrap()
      .status,
    "cancel_requested"
  );
  ui.click(
    cx,
    gpui_kit::SharedString::from(format!("task-reconcile-{third}")),
  );
  ui.wait(cx, || {
    remote
      .pair
      .master
      .store
      .task(&third)
      .ok()
      .flatten()
      .is_some_and(|task| task.status == "cancelled")
  })
  .await;
  assert_eq!(
    remote
      .pair
      .worker
      .store
      .task(&third)
      .expect("worker read")
      .expect("worker task")
      .status,
    "cancelled"
  );

  assert_eq!(
    runtime.block_on(mock.requests()).len(),
    3,
    "each explicit task executes once"
  );
  runtime.block_on(remote.close());
}

#[gpui_kit::test]
async fn uncertain_dispatch_keeps_identity_and_does_not_create_a_fresh_task(
  cx: &mut TestAppContext,
) {
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(2)
    .enable_all()
    .build()
    .expect("remote fixture runtime");
  let mock = runtime
    .block_on(MockModelServer::start(vec![]))
    .expect("synthetic model server");
  let mut remote = runtime.block_on(Remote::open(&mock, StandardProtocol::ChatCompletions));
  let _parent = remote.pair.envelope(vec!["workspace.read".into()]);
  remote
    .pair
    .master
    .store
    .create_session(
      "Other chat",
      &remote.pair.session.workspace,
      &remote.pair.master.node_id,
    )
    .expect("second session for tab switch");
  let target = worker_target(&remote);
  let ui = TaskUi::new(cx, &remote.endpoint, remote.pair.master.auth_token());
  ui.connect(cx);
  ui.wait_connected(cx).await;
  ui.select_session_and_open_tasks_at(cx, 1);
  ui.wait_active_run(cx).await;
  ui.set_target(cx, &target.id, &target.endpoint);
  runtime.block_on(remote.pair.abort_worker_listener());

  ui.click(cx, "task-dispatch");
  ui.wait_element(cx, "task-error").await;
  // The Worker is unreachable before the dispatch handshake, so the daemon
  // has not recorded an execution. The client still retains its original ID.
  assert!(
    remote
      .pair
      .master
      .store
      .tasks()
      .expect("no dispatch effect")
      .is_empty()
  );
  ui.click(cx, "task-intent-inspect");
  ui.wait(cx, || true).await;
  ui.click(cx, ("session", 0usize));
  let other_session = remote.pair.master.store.sessions().expect("sessions")[0]
    .id
    .clone();
  ui.wait_selected_session(cx, &other_session).await;
  ui.click(cx, ("session", 1usize));
  ui.wait_selected_session(cx, &remote.pair.session.id).await;
  ui.wait_active_run(cx).await;
  ui.pump(cx).await;
  cx.update_window(ui.window.into(), |_, window, cx| {
    assert!(window.find("task-dispatch-intent").visible());
    window.click("task-dispatch", cx);
  })
  .expect("unresolved dispatch remains visible and guarded");
  ui.pump(cx).await;
  assert_eq!(
    remote
      .pair
      .master
      .store
      .tasks()
      .expect("single original task")
      .len(),
    0,
    "unknown outcome must not create a replacement identity"
  );
  assert_eq!(
    runtime.block_on(mock.requests()).len(),
    0,
    "unreachable Worker never starts paid or synthetic model execution"
  );
  runtime.block_on(remote.close());
}

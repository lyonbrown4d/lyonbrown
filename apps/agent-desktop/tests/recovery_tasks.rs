#![cfg(unix)]
use gpui_kit::{AppContext as _, TestAppContext, test::TestWindowExt as _};

#[path = "../../agentd/tests/peer_support/mod.rs"]
#[allow(dead_code)]
mod peer_support;
mod recovery_support;
#[path = "../../agentd/tests/remote_support/mod.rs"]
#[allow(dead_code)]
mod remote_support;
#[path = "support/tasks.rs"]
#[allow(dead_code)]
mod task_support;

#[gpui_kit::test]
async fn desktop_verifies_real_effect_and_worker_before_fresh_continuation(
  cx: &mut TestAppContext,
) {
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(3)
    .enable_all()
    .build()
    .unwrap();
  let scenario = runtime.block_on(recovery_support::Scenario::open(5000));
  let remote = &scenario.remote;
  let ui = task_support::TaskUi::new(cx, &remote.endpoint, remote.pair.master.auth_token());
  ui.connect(cx);
  ui.wait_connected(cx).await;
  ui.click(cx, ("session", 0usize));
  ui.wait_element(cx, ("allow", 0usize)).await;
  assert!(!scenario.effect().exists());
  ui.click(cx, ("allow", 0usize));
  ui.wait_element(
    cx,
    gpui_kit::SharedString::from(format!("recover-{}", scenario.original.id)),
  )
  .await;
  assert_eq!(
    std::fs::read_to_string(scenario.effect()).unwrap(),
    "effect\n"
  );
  let before = remote
    .pair
    .master
    .store
    .run(&scenario.original.id)
    .unwrap()
    .unwrap();
  assert_eq!(before.status, "uncertain");
  ui.wait(cx, || {
    scenario.dispatch.is_finished()
      && remote
        .pair
        .master
        .store
        .task(&scenario.task_id)
        .unwrap()
        .is_some_and(|task| task.status == "awaiting_acceptance")
  })
  .await;
  ui.click(
    cx,
    gpui_kit::SharedString::from(format!("recover-{}", scenario.original.id)),
  );
  wait_recovery(&ui, cx, |state| state.inspected).await;
  cx.update(|cx| {
    assert!(
      ui.view
        .read(cx)
        .snapshot(cx)
        .recovery
        .unwrap()
        .allowed_actions
        .is_empty()
    )
  });
  assert!(
    remote
      .pair
      .master
      .store
      .run_recovery(&scenario.original.id)
      .unwrap()
      .is_none()
  );
  ui.click(cx, "recovery-tasks");
  ui.wait_element(
    cx,
    gpui_kit::SharedString::from(format!("task-reconcile-{}", scenario.task_id)),
  )
  .await;
  assert!(cx.update(|cx| ui.view.read(cx).snapshot(cx).workbench.recovery_tasks_only));
  let target = remote.target();
  ui.set_target(cx, &target.id, &target.endpoint);
  ui.click(
    cx,
    gpui_kit::SharedString::from(format!("task-reconcile-{}", scenario.task_id)),
  );
  ui.wait(cx, || {
    remote
      .pair
      .master
      .store
      .task(&scenario.task_id)
      .unwrap()
      .unwrap()
      .status
      == "awaiting_acceptance"
  })
  .await;
  ui.click(
    cx,
    gpui_kit::SharedString::from(format!("task-accept-{}", scenario.task_id)),
  );
  ui.wait(cx, || {
    remote
      .pair
      .master
      .store
      .task(&scenario.task_id)
      .unwrap()
      .unwrap()
      .status
      == "completed"
  })
  .await;
  ui.click(cx, "toggle-workbench");
  ui.click(cx, "recovery-refresh");
  wait_recovery(&ui, cx, |state| {
    state.inspected && state.allowed_actions.len() == 2
  })
  .await;
  // Empty verification must not admit a recovery or call a model.
  ui.click(cx, "recovery-continue");
  assert!(
    remote
      .pair
      .master
      .store
      .run_recovery(&scenario.original.id)
      .unwrap()
      .is_none()
  );
  cx.update_window(ui.window.into(), |_, window, cx| {
    window.click("recovery-note", cx);
    window.input("已检查 effect.txt 仅一行，并采纳 Worker 结果。", cx);
    window.click("recovery-prompt", cx);
    window.input("只汇总已核对结果，不再执行命令。", cx);
  })
  .unwrap();
  ui.click(cx, "recovery-continue");
  wait_recovery(&ui, cx, |state| state.resolved).await;
  let resolution = remote
    .pair
    .master
    .store
    .run_recovery(&scenario.original.id)
    .unwrap()
    .unwrap();
  let fresh_id = resolution.session_id.clone().unwrap();
  ui.click(cx, "recovery-open-session");
  ui.wait_selected_session(cx, &fresh_id).await;
  ui.wait(cx, || {
    remote
      .pair
      .master
      .store
      .run(resolution.replacement_run_id.as_ref().unwrap())
      .unwrap()
      .unwrap()
      .status
      == "completed"
  })
  .await;
  assert_eq!(
    remote.pair.master.store.run(&scenario.original.id).unwrap(),
    Some(before)
  );
  assert_eq!(
    std::fs::read_to_string(scenario.effect()).unwrap(),
    "effect\n"
  );
  let requests = runtime.block_on(scenario.model.requests());
  assert_eq!(requests.len(), 2);
  assert!(requests[1].body.to_string().contains("只汇总已核对结果"));
  assert!(!requests[1].body.to_string().contains("original-command"));
  assert!(!requests[1].body.to_string().contains("exec /bin/sleep"));
  assert_eq!(runtime.block_on(scenario.worker_model.requests()).len(), 1);
  let event = remote
    .pair
    .master
    .store
    .events(&scenario.original.id, 0, 1000)
    .unwrap()
    .into_iter()
    .find(|event| event.kind == "run_recovery_recorded")
    .unwrap();
  assert!(
    remote
      .pair
      .master
      .store
      .pending_outbox(1000)
      .unwrap()
      .iter()
      .any(|pending| pending.run_id == event.run_id && pending.cursor == event.cursor)
  );
  assert_eq!(
    remote
      .pair
      .master
      .store
      .session(&scenario.original.session_id)
      .unwrap()
      .unwrap()
      .owner_epoch,
    2
  );
  let repeat = runtime
    .block_on(
      remote
        .client
        .recover_run(agent_protocol::v1::RecoverRunRequest {
          run_id: scenario.original.id.clone(),
          owner_epoch: resolution.requested_owner_epoch,
          action: agent_protocol::v1::RunRecoveryAction::ContinueNewSession as i32,
          idempotency_key: resolution.idempotency_key,
          note: resolution.note,
          prompt: resolution.prompt,
        }),
    )
    .unwrap();
  assert_eq!(repeat.session.unwrap().id, fresh_id);
  assert_eq!(runtime.block_on(scenario.model.requests()).len(), 2);
  let evidence = runtime.block_on(scenario.evidence()).unwrap();
  assert_eq!(evidence["original_run"]["status"], "uncertain");
  assert_eq!(evidence["replacement_run"]["status"], "completed");
  assert_eq!(evidence["effect"], "effect\n");
  runtime.block_on(scenario.remote.close());
}

async fn wait_recovery(
  ui: &task_support::TaskUi,
  cx: &mut TestAppContext,
  ready: impl Fn(&agent_desktop::RecoverySnapshot) -> bool,
) {
  let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
  loop {
    let done = cx.update(|cx| {
      ui.view
        .read(cx)
        .snapshot(cx)
        .recovery
        .is_some_and(|state| !state.pending && ready(&state))
    });
    if done {
      return;
    }
    assert!(std::time::Instant::now() < deadline, "recovery timed out");
    ui.pump(cx).await;
  }
}

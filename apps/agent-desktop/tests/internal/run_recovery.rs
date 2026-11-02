use crate::{connection::Update, test_support::Harness};
use agent_core::RunStatus;
use agent_protocol::v1;
use agent_test_support::{StandardProtocol, text_reply};
use gpui_kit::{AppContext as _, SharedString, TestAppContext, test::TestWindowExt as _};

async fn interrupted(harness: &Harness) -> agent_core::Run {
  let session = harness
    .runtime
    .block_on(harness.client.create_session(
      "Interrupted",
      harness.directory.path().join("workspace").to_str().unwrap(),
    ))
    .unwrap();
  let (run, _) = harness
    .daemon
    .store
    .accept_run(&session.id, 1, "never replay this original", "original")
    .await
    .unwrap();
  harness
    .daemon
    .store
    .update_run_owned(&run.id, 1, RunStatus::Uncertain, "", "fixture interruption")
    .await
    .unwrap()
}

async fn open(harness: &Harness, run: &agent_core::Run, cx: &mut TestAppContext) {
  harness.connect(cx);
  harness
    .wait(cx, async |state| state.local_state_loaded)
    .await;
  harness.click(("session", 0usize), cx);
  harness
    .wait(cx, async |state| {
      !state.busy && state.selected_session.as_deref() == Some(&run.session_id)
    })
    .await;
  harness.click(SharedString::from(format!("recover-{}", run.id)), cx);
  harness
    .wait(cx, async |state| {
      state
        .recovery
        .as_ref()
        .is_some_and(|state| state.inspected && !state.pending)
    })
    .await;
}

#[gpui_kit::test]
async fn abandon_records_only_and_preserves_original_chat_draft(cx: &mut TestAppContext) {
  let harness = Harness::new(cx, Vec::new());
  let storage_runtime_0 = harness.runtime.clone();
  let _storage_context_0 = storage_runtime_0.enter();
  let run = interrupted(&harness).await;
  open(&harness, &run, cx).await;
  cx.update_window(harness.window.into(), |_, window, cx| {
    harness
      .view
      .read(cx)
      .composer
      .clone()
      .update(cx, |input, cx| {
        input.set_value("keep original draft", window, cx)
      });
    window.click("recovery-note", cx);
    window.input("Verified the fixture; leave it stopped.", cx);
  })
  .unwrap();
  harness.click("recovery-abandon", cx);
  harness
    .wait(cx, async |state| {
      state.recovery.as_ref().is_some_and(|state| state.resolved)
    })
    .await;
  let resolution = harness
    .daemon
    .store
    .run_recovery(&run.id)
    .await
    .unwrap()
    .unwrap();
  assert!(resolution.session_id.is_none());
  assert!(resolution.replacement_run_id.is_none());
  assert_eq!(harness.daemon.store.sessions().await.unwrap().len(), 1);
  assert_eq!(harness.daemon.store.run(&run.id).await.unwrap(), Some(run));
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty()
  );
  harness.click("recovery-close", cx);
  assert_eq!(harness.snapshot(cx).draft, "keep original draft");
}

#[gpui_kit::test]
async fn lost_recovery_reply_requires_inspection_and_never_submits_again(cx: &mut TestAppContext) {
  let harness = Harness::new(
    cx,
    vec![text_reply(
      StandardProtocol::ChatCompletions,
      "fresh result",
    )],
  );
  let storage_runtime_1 = harness.runtime.clone();
  let _storage_context_1 = storage_runtime_1.enter();
  let run = interrupted(&harness).await;
  open(&harness, &run, cx).await;
  let request = v1::RecoverRunRequest {
    run_id: run.id.clone(),
    owner_epoch: 1,
    action: v1::RunRecoveryAction::ContinueNewSession as i32,
    idempotency_key: "lost-reply".into(),
    note: "Verified previous effects".into(),
    prompt: "fresh summary only".into(),
  };
  // Commit at the real daemon, deliberately discard its result, then deliver
  // the transport-error presentation that the requesting tab would receive.
  let result = harness
    .runtime
    .block_on(harness.client.recover_run(request.clone()))
    .unwrap();
  cx.update_window(harness.window.into(), |_, window, cx| {
    harness.view.update(cx, |view, cx| {
      let state = view.vm.recovery.as_mut().unwrap();
      state.intent = Some(request);
      state.pending = true;
      view.new_chat_tab(window, cx);
      assert_eq!(view.vm.selected.as_deref(), Some(run.session_id.as_str()));
      view.apply(
        Update::RecoveryError(run.id.clone(), "lost response".into()),
        window,
        cx,
        true,
      );
    });
    window.render_frame(cx);
  })
  .unwrap();
  assert!(!harness.snapshot(cx).recovery.unwrap().inspected);
  harness.click("recovery-continue", cx);
  assert_eq!(harness.daemon.store.sessions().await.unwrap().len(), 2);
  harness.click("recovery-refresh", cx);
  harness
    .wait(cx, async |state| {
      state.recovery.as_ref().is_some_and(|state| state.resolved)
    })
    .await;
  let state = harness.snapshot(cx);
  assert_eq!(
    state.selected_session.as_deref(),
    Some(run.session_id.as_str())
  );
  assert_eq!(
    state.recovery.unwrap().replacement_session,
    result.session.map(|session| session.id)
  );
  harness.click("recovery-open-session", cx);
  harness
    .wait(cx, async |state| {
      state
        .messages
        .iter()
        .any(|(_, text)| text.contains("fresh result"))
    })
    .await;
  assert_eq!(harness.runtime.block_on(harness.model.requests()).len(), 1);
  assert_eq!(harness.daemon.store.run(&run.id).await.unwrap(), Some(run));
}

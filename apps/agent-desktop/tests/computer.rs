mod support;

use gpui_kit::{AppContext as _, ScrollDelta, TestAppContext, point, px, test::TestWindowExt as _};
use std::sync::{Arc, atomic::Ordering};
use support::{Harness, computer::Factory};

fn show_computer(harness: &Harness, cx: &mut TestAppContext) {
  harness.show_workbench(cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    // The maintained dock strip scrolls when its tabs exceed the panel width.
    // Reveal Computer with the same horizontal wheel gesture as a native user.
    window
      .within("tab-bar")
      .scroll(0usize, ScrollDelta::Pixels(point(px(-600.), px(0.))), cx);
    window.within("tab-bar").click(5usize, cx);
  })
  .expect("select computer dock pane");
}

fn select_scope(harness: &Harness, cx: &mut TestAppContext) {
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.within("computer-scope").click("input", cx);
    window.press("down", cx);
    window.press("enter", cx);
  })
  .expect("explicit scope selection");
}

#[gpui_kit::test]
async fn explicit_scope_attach_stays_with_original_chat_and_detach_closes_real_host(
  cx: &mut TestAppContext,
) {
  let factory = Arc::new(Factory::default());
  let harness = Harness::with_computer(cx, factory.clone());
  harness.connect(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  harness.click("new-session", cx);
  let initial = harness
    .wait(cx, |state| state.selected_session.is_some() && !state.busy)
    .await;
  let original = initial.selected_session.expect("selected session");
  show_computer(&harness, cx);
  assert!(!harness.snapshot(cx).workbench.computer.attached);
  assert_eq!(factory.attached.load(Ordering::SeqCst), 0);
  harness.click("computer-refresh", cx);
  let catalog = harness
    .wait(cx, |state| {
      !state.workbench.computer.pending && !state.workbench.computer.targets.is_empty()
    })
    .await;
  assert!(catalog.workbench.computer.selected_scope.is_none());
  harness.click("computer-attach", cx);
  assert_eq!(
    factory.attached.load(Ordering::SeqCst),
    0,
    "catalog does not authorize an attachment"
  );
  select_scope(&harness, cx);
  harness
    .wait(cx, |state| {
      state.workbench.computer.selected_scope.is_some()
    })
    .await;
  harness.click("computer-attach", cx);
  let attached = harness
    .wait(cx, |state| state.workbench.computer.attached)
    .await;
  assert_eq!(
    attached.workbench.computer.bound_session.as_deref(),
    Some(original.as_str())
  );
  let registered = harness
    .runtime
    .block_on(harness.daemon.computer_runtime_info(&original))
    .expect("host lookup")
    .expect("actual authenticated host");
  assert_eq!(
    registered.desktop_session.scope,
    agent_computer::TargetScope::Window(7)
  );
  assert!(!format!("{:?}", attached.workbench.computer).contains("Private fixture"));
  harness.click("new-chat-tab", cx);
  let other = harness
    .wait(cx, |state| {
      state
        .selected_session
        .as_deref()
        .is_some_and(|id| id != original)
        && !state.busy
    })
    .await;
  assert_eq!(
    other.workbench.computer.bound_session.as_deref(),
    Some(original.as_str())
  );
  assert!(other.workbench.computer.selected_scope.is_none());
  assert_eq!(
    factory.attached.load(Ordering::SeqCst),
    1,
    "switching never transfers or attaches"
  );
  harness.click("computer-detach", cx);
  harness
    .wait(cx, |state| {
      state.workbench.computer.bound_session.is_none()
        && factory.dropped.load(Ordering::SeqCst) == 1
    })
    .await;
  harness
    .wait(cx, |_| {
      harness
        .runtime
        .block_on(harness.daemon.computer_runtime_info(&original))
        .expect("host lookup")
        .is_none()
    })
    .await;
  assert_eq!(factory.attached.load(Ordering::SeqCst), 1);
  // Attach to the second chat only after another explicit refresh and selection.
  harness.click("computer-refresh", cx);
  harness
    .wait(cx, |state| {
      !state.workbench.computer.pending && !state.workbench.computer.targets.is_empty()
    })
    .await;
  select_scope(&harness, cx);
  harness
    .wait(cx, |state| {
      state.workbench.computer.selected_scope.is_some()
    })
    .await;
  harness.click("computer-attach", cx);
  harness
    .wait(cx, |state| state.workbench.computer.attached)
    .await;
  harness.connect(cx);
  harness
    .wait(cx, |state| {
      state.selected_session == other.selected_session
        && !state.workbench.computer.attached
        && factory.dropped.load(Ordering::SeqCst) == 2
    })
    .await;
  let restored = harness.snapshot(cx).workbench.computer;
  assert!(restored.bound_session.is_none());
  assert!(restored.selected_scope.is_none());
  assert!(restored.targets.is_empty());
  assert_eq!(
    factory.attached.load(Ordering::SeqCst),
    2,
    "reconnect must not autoattach"
  );
}

#[cfg(not(feature = "native-computer"))]
#[gpui_kit::test]
async fn build_without_native_host_reports_unavailable_and_never_offers_a_default_scope(
  cx: &mut TestAppContext,
) {
  let harness = Harness::new(cx, Vec::new());
  show_computer(&harness, cx);
  let state = harness.snapshot(cx).workbench.computer;
  assert!(!state.available);
  assert!(!state.attached);
  assert!(state.selected_scope.is_none());
  harness.click("computer-refresh", cx);
  harness.click("computer-attach", cx);
  assert!(!harness.snapshot(cx).workbench.computer.pending);
}

async fn attached_host(cx: &mut TestAppContext) -> (Harness, Arc<Factory>, String) {
  let factory = Arc::new(Factory::default());
  let harness = Harness::with_computer(cx, factory.clone());
  harness.connect(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  harness.click("new-session", cx);
  let session = harness
    .wait(cx, |state| state.selected_session.is_some() && !state.busy)
    .await
    .selected_session
    .expect("selected chat");
  show_computer(&harness, cx);
  harness.click("computer-refresh", cx);
  harness
    .wait(cx, |state| {
      !state.workbench.computer.pending && !state.workbench.computer.targets.is_empty()
    })
    .await;
  select_scope(&harness, cx);
  harness
    .wait(cx, |state| {
      state.workbench.computer.selected_scope.is_some()
    })
    .await;
  harness.click("computer-attach", cx);
  harness
    .wait(cx, |state| state.workbench.computer.attached)
    .await;
  (harness, factory, session)
}

#[gpui_kit::test]
async fn failed_reconnect_immediately_revokes_host_and_does_not_restore_authority(
  cx: &mut TestAppContext,
) {
  let (harness, factory, session) = attached_host(cx).await;
  harness.click("open-settings", cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("access-token", cx);
    window.press(&support::shortcut("a"), cx);
    window.input("invalid-fixture-token", cx);
    window.click("connect", cx);
    let state = harness.view.read(cx).snapshot(cx).workbench.computer;
    assert!(
      !state.attached,
      "dial attempt revokes before async authentication"
    );
    assert!(state.bound_session.is_none());
    assert!(state.bound_scope.is_none());
    assert!(state.targets.is_empty());
    assert!(state.selected_scope.is_none());
  })
  .expect("explicit failed reconnect");
  harness
    .wait(cx, |state| {
      state.status == "Connection or request failed"
        && factory.dropped.load(Ordering::SeqCst) == 1
        && harness
          .runtime
          .block_on(harness.daemon.computer_runtime_info(&session))
          .expect("host lookup")
          .is_none()
    })
    .await;
  let state = harness.snapshot(cx).workbench.computer;
  assert!(!state.attached);
  assert!(state.bound_session.is_none());
  assert!(state.selected_scope.is_none());
  assert_eq!(
    factory.attached.load(Ordering::SeqCst),
    1,
    "failure never reattaches"
  );
}

#[gpui_kit::test]
async fn detach_after_reverse_channel_failure_clears_binding_and_warns_that_native_work_may_drain(
  cx: &mut TestAppContext,
) {
  let (harness, factory, _) = attached_host(cx).await;
  // Ending the real daemon reverse channel completes the host with an error.
  harness.runtime.block_on(harness.daemon.shutdown());
  harness
    .wait(cx, |state| {
      !state.workbench.computer.attached
        && state.workbench.computer.error.is_some()
        && factory.dropped.load(Ordering::SeqCst) == 1
    })
    .await;
  harness.click("computer-detach", cx);
  let state = harness
    .wait(cx, |state| {
      state.workbench.computer.bound_session.is_none() && !state.workbench.computer.pending
    })
    .await
    .workbench
    .computer;
  assert!(state.detach_warning);
  assert!(!state.attached);
  assert!(state.bound_scope.is_none());
  let warning = state.error.expect("shutdown failure warning");
  assert!(warning.contains("may still be draining"));
  assert!(warning.contains("No actions were replayed"));
  assert!(!warning.contains("Private fixture"));
  assert_eq!(factory.attached.load(Ordering::SeqCst), 1);
}

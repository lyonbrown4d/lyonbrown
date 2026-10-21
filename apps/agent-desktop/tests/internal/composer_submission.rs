use crate::test_support as support;
use agent_test_support::{StandardProtocol, text_reply};
use gpui_kit::{
  AppContext as _, EntityInputHandler as _, TestAppContext, test::TestWindowExt as _,
};

#[gpui_kit::test]
async fn native_enter_sends_shift_enter_edits_and_ime_composition_never_submits(
  cx: &mut TestAppContext,
) {
  let harness = support::Harness::new(
    cx,
    vec![
      text_reply(StandardProtocol::ChatCompletions, "First native submission"),
      text_reply(
        StandardProtocol::ChatCompletions,
        "Second native submission",
      ),
    ],
  );
  harness.connect(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("l"), cx);
    window.press("enter", cx);
  })
  .expect("empty Enter");
  assert!(harness.snapshot(cx).selected_session.is_none());

  let composer = cx.update(|cx| harness.view.read(cx).composer.clone());
  cx.update_window(harness.window.into(), |_, window, cx| {
    composer.update(cx, |input, cx| {
      input.replace_and_mark_text_in_range(None, "ni", Some(0..2), window, cx);
    });
    window.press("enter", cx);
  })
  .expect("Enter during composition");
  assert!(!harness.snapshot(cx).busy);
  assert!(harness.snapshot(cx).selected_session.is_none());
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty()
  );

  cx.update_window(harness.window.into(), |_, window, cx| {
    composer.update(cx, |input, cx| {
      input.replace_text_in_range(None, "你好", window, cx);
    });
    window.press("shift-enter", cx);
    window.input("第二行", cx);
  })
  .expect("commit Chinese text and insert a newline");
  assert_eq!(harness.snapshot(cx).draft, "你好\n第二行");
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty()
  );
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press("enter", cx);
  })
  .expect("plain Enter submits committed multiline text");
  let completed = harness
    .wait(cx, |state| {
      state.active_run.is_none()
        && state
          .messages
          .iter()
          .any(|(_, text)| text.contains("First native"))
    })
    .await;
  assert!(completed.draft.is_empty());
  let session = completed.selected_session.expect("created session");
  let runs = harness
    .runtime
    .block_on(harness.client.list_runs(&session))
    .expect("persisted run");
  assert_eq!(runs.len(), 1);
  assert_eq!(runs[0].prompt, "你好\n第二行");

  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("t"), cx);
  })
  .expect("create new chat tab");
  harness
    .wait(cx, |state| {
      state.tabs.len() == 2 && state.selected_session.is_some() && !state.busy
    })
    .await;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("l"), cx);
    window.input("新标签也使用 Enter 发送", cx);
    window.press("enter", cx);
  })
  .expect("new-tab composer uses the same component submission mode");
  let completed = harness
    .wait(cx, |state| {
      state.active_run.is_none()
        && state
          .messages
          .iter()
          .any(|(_, text)| text.contains("Second native"))
    })
    .await;
  assert!(completed.draft.is_empty());
  assert_eq!(harness.runtime.block_on(harness.model.requests()).len(), 2);
}

mod support;

use agent_test_support::{ScriptedReply, StandardProtocol, text_reply, tool_reply};
use gpui_kit::{AppContext as _, TestAppContext, test::TestWindowExt as _};
use serde_json::json;
use support::Harness;

fn draft(harness: &Harness, cx: &mut TestAppContext, text: &str) {
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("l"), cx);
    window.input(text, cx);
  })
  .expect("type in native composer");
}

#[gpui_kit::test]
async fn independent_drafts_and_workspace_survive_switch_and_last_tab_close(
  cx: &mut TestAppContext,
) {
  let harness = Harness::new(cx, Vec::new());
  let first = harness.snapshot(cx).tabs[0].id;
  draft(&harness, cx, "First unsent draft");
  let first_input = cx.update(|cx| harness.view.read(cx).composer_input_id());
  harness.click("new-chat-tab", cx);
  let second = harness.snapshot(cx).tabs[1].id;
  assert_eq!(harness.snapshot(cx).draft, "");
  draft(&harness, cx, "Second unsent draft");
  harness.click("open-settings", cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("workspace", cx);
    window.press(&support::shortcut("a"), cx);
    window.input("/different/workspace", cx);
  })
  .expect("change second tab workspace");
  harness.click("close-settings", cx);
  harness.click(("chat-tab", first), cx);
  assert_eq!(harness.snapshot(cx).draft, "First unsent draft");
  cx.update_window(harness.window.into(), |_, window, cx| {
    assert_eq!(harness.view.read(cx).composer_input_id(), first_input);
    // TabBar retains its accessible tab focus; the composer shortcut restores
    // the same native editor when the user wants to resume typing.
    window.press(&support::shortcut("l"), cx);
    assert_eq!(window.find(first_input).focused(), Some(true));
  })
  .expect("original native input restored");
  harness.click("open-settings", cx);
  cx.update_window(harness.window.into(), |_, window, _| {
    assert_eq!(
      window.find("workspace").value(),
      Some(
        harness
          .directory
          .path()
          .join("workspace")
          .to_str()
          .expect("workspace path")
      )
    );
  })
  .expect("original workspace restored");
  harness.click("close-settings", cx);
  harness.click(("chat-tab", second), cx);
  assert_eq!(harness.snapshot(cx).draft, "Second unsent draft");
  harness.click(("close-chat-tab", first), cx);
  assert_eq!(harness.snapshot(cx).draft, "Second unsent draft");
  harness.click(("close-chat-tab", second), cx);
  let blank = harness.snapshot(cx);
  assert_eq!(blank.tabs.len(), 1);
  assert_ne!(blank.tabs[0].id, first);
  assert_ne!(blank.tabs[0].id, second);
  assert!(blank.tabs[0].selected);
  assert_eq!(blank.draft, "");
  assert!(blank.selected_session.is_none());
}

#[gpui_kit::test]
async fn background_session_ack_approval_and_completion_stay_in_origin_tab_and_close_keeps_run(
  cx: &mut TestAppContext,
) {
  let harness = Harness::new(
    cx,
    vec![
      tool_reply(
        StandardProtocol::ChatCompletions,
        "write_file",
        json!({"path":"tab-a.txt","content":"approved in A"}),
      ),
      text_reply(StandardProtocol::ChatCompletions, "Only chat A completed."),
      ScriptedReply::stall(),
    ],
  );
  harness.connect(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  let first = harness.snapshot(cx).tabs[0].id;
  harness.input_prompt(cx, "Write tab-a.txt after approval.");
  // Switch before the poll delivers Session/Started acknowledgements.
  harness.click("new-chat-tab", cx);
  let second = harness.snapshot(cx).tabs[1].id;
  let background = harness
    .wait(cx, |state| {
      !state.busy
        && state.selected_session.is_some()
        && state
          .tabs
          .iter()
          .any(|tab| tab.id == first && tab.active_run.is_some())
    })
    .await;
  assert!(background.messages.is_empty());
  assert!(background.approvals.is_empty());
  assert!(background.active_run.is_none());
  let first_tab = background
    .tabs
    .iter()
    .find(|tab| tab.id == first)
    .expect("origin tab");
  let run_id = first_tab.active_run.clone().expect("origin run");
  let persisted = harness
    .runtime
    .block_on(harness.client.get_run(&run_id))
    .expect("persisted origin run");
  assert_eq!(Some(persisted.session_id), first_tab.session_id.clone());
  assert_ne!(first_tab.session_id, background.selected_session);
  draft(&harness, cx, "Unsent chat B draft");
  harness.click(("chat-tab", first), cx);
  let approval = harness.wait(cx, |state| !state.approvals.is_empty()).await;
  assert_eq!(approval.approvals[0].1, "write_file");
  assert!(
    !harness
      .directory
      .path()
      .join("workspace/tab-a.txt")
      .exists()
  );
  harness.click(("allow", 0usize), cx);
  harness.click(("chat-tab", second), cx);
  let completed = harness
    .wait(cx, |state| {
      state
        .tabs
        .iter()
        .any(|tab| tab.id == first && tab.active_run.is_none())
    })
    .await;
  assert_eq!(completed.draft, "Unsent chat B draft");
  assert!(completed.messages.is_empty());
  assert!(completed.approvals.is_empty());
  assert_eq!(
    std::fs::read_to_string(harness.directory.path().join("workspace/tab-a.txt"))
      .expect("approved write"),
    "approved in A"
  );
  harness.click(("chat-tab", first), cx);
  let origin = harness.snapshot(cx);
  assert!(
    origin
      .messages
      .iter()
      .any(|(_, body)| body.contains("Only chat A completed"))
  );
  assert_eq!(
    origin
      .messages
      .iter()
      .filter(|(role, _)| role == "You")
      .count(),
    1
  );
  assert_eq!(
    harness
      .runtime
      .block_on(harness.client.get_run(&run_id))
      .expect("terminal run")
      .status,
    "completed"
  );

  harness.click(("chat-tab", second), cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("l"), cx);
    window.press(&support::shortcut("a"), cx);
  })
  .expect("replace B draft");
  harness.input_prompt(cx, "Keep running after I close this tab.");
  let running = harness.wait(cx, |state| state.active_run.is_some()).await;
  let running_id = running.active_run.expect("B daemon run");
  harness.runtime.block_on(async {
    tokio::time::timeout(
      std::time::Duration::from_secs(3),
      harness.model.wait_for_requests(3),
    )
    .await
    .expect("stalled model request");
  });
  harness.click(("chat-tab", first), cx);
  harness.click(("close-chat-tab", second), cx);
  let kept = harness
    .runtime
    .block_on(harness.client.get_run(&running_id))
    .expect("daemon retains closed tab run");
  assert!(!matches!(
    kept.status.as_str(),
    "completed" | "failed" | "cancelled" | "uncertain"
  ));
  let closed = harness.snapshot(cx);
  assert_eq!(closed.tabs.len(), 1);
  assert_eq!(closed.messages, origin.messages);
  // Cleanup is an explicit daemon action; closing the view never issues this command.
  harness
    .runtime
    .block_on(harness.client.cancel_run(&running_id))
    .expect("explicit fixture cleanup");
  let after = harness
    .wait(cx, |state| {
      state.tabs.len() == 1 && state.active_run.is_none()
    })
    .await;
  assert_eq!(after.messages, origin.messages);
  assert_eq!(
    harness.runtime.block_on(harness.model.requests()).len(),
    3,
    "switching and reopening must not replay input"
  );
}

#[gpui_kit::test]
async fn sidebar_reuses_open_session_tab_without_losing_other_draft(cx: &mut TestAppContext) {
  let harness = Harness::new(cx, Vec::new());
  let session = harness
    .runtime
    .block_on(
      harness.client.create_session(
        "Saved chat",
        harness
          .directory
          .path()
          .join("workspace")
          .to_str()
          .expect("workspace path"),
      ),
    )
    .expect("saved session");
  harness.connect(cx);
  harness
    .wait(cx, |state| state.session_titles.len() == 1)
    .await;
  let draft_id = harness.snapshot(cx).tabs[0].id;
  draft(&harness, cx, "Keep this draft");
  harness.click(("session", 0usize), cx);
  let loaded = harness
    .wait(cx, |state| {
      state.selected_session.as_deref() == Some(session.id.as_str()) && !state.busy
    })
    .await;
  let saved_id = loaded
    .tabs
    .iter()
    .find(|tab| tab.selected)
    .expect("selected saved chat")
    .id;
  assert_eq!(loaded.tabs.len(), 2);
  harness.click(("chat-tab", draft_id), cx);
  assert_eq!(harness.snapshot(cx).draft, "Keep this draft");
  harness.click(("session", 0usize), cx);
  let reopened = harness.snapshot(cx);
  assert_eq!(reopened.tabs.len(), 2);
  assert_eq!(
    reopened
      .tabs
      .iter()
      .find(|tab| tab.selected)
      .expect("same selected chat")
      .id,
    saved_id
  );
  assert_eq!(
    reopened.selected_session.as_deref(),
    Some(session.id.as_str())
  );
}

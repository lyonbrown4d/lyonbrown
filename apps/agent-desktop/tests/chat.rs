use gpui_kit::AppContext as _;
mod support;

use agent_test_support::{ScriptedReply, StandardProtocol, text_reply, tool_reply};
use gpui_kit::{TestAppContext, component::ActiveTheme as _, px, size, test::TestWindowExt as _};
use serde_json::json;
use support::Harness;

#[gpui_kit::test]
async fn real_keyboard_submission_streams_markdown_and_loads_sidebar_history(
  cx: &mut TestAppContext,
) {
  let harness = Harness::new(
    cx,
    vec![text_reply(
      StandardProtocol::ChatCompletions,
      "## Complete\n\nA **native** response.",
    )],
  );
  harness
    .runtime
    .block_on(harness.client.create_session(
      "Existing project",
      harness.directory.path().to_str().expect("test path"),
    ))
    .expect("existing session");
  harness.connect(cx);
  harness
    .wait(cx, |snapshot| {
      snapshot.status.starts_with("Connected") && snapshot.session_titles.len() == 1
    })
    .await;
  harness.click(("session", 0usize), cx);
  harness
    .wait(cx, |snapshot| {
      snapshot.selected_session.is_some() && snapshot.error.is_none() && !snapshot.busy
    })
    .await;
  harness.click("new-session", cx);
  harness
    .wait(cx, |snapshot| snapshot.session_titles.len() == 2)
    .await;
  harness.input_prompt(cx, "Explain the code in markdown.");
  let complete = harness
    .wait(cx, |snapshot| {
      snapshot.active_run.is_none()
        && snapshot
          .messages
          .iter()
          .any(|(_, body)| body.contains("native"))
    })
    .await;
  assert_eq!(complete.draft, "");
  assert_eq!(
    complete
      .messages
      .iter()
      .filter(|(role, _)| role == "You")
      .count(),
    1
  );
  let requests = harness.runtime.block_on(harness.model.requests());
  assert_eq!(requests.len(), 1, "UI must not duplicate the model prompt");
  cx.update_window(harness.window.into(), |_, window, cx| {
    assert!(
      window.try_find("access-token").is_none(),
      "credentials are outside the chat view"
    );
    window.press(&support::shortcut("l"), cx);
    assert_eq!(
      window
        .find(harness.view.read(cx).composer_input_id())
        .focused(),
      Some(true)
    );
    let before = cx.theme().is_dark();
    window.click("open-settings", cx);
    window.click(if before { "theme-light" } else { "theme-dark" }, cx);
    window.click("close-settings", cx);
    assert_ne!(cx.theme().is_dark(), before);
    assert!(window.find("sidebar").bounds().size.width < px(350.));
    window.click("toggle-workbench", cx);
    assert!(window.find("composer").visible());
  })
  .expect("native theme, focus and layout interactions");
  cx.simulate_window_resize(harness.window.into(), size(px(960.), px(640.)));
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.render_frame(cx);
    assert!(window.find("composer").visible());
    assert!(window.find("composer").bounds().bottom() <= window.viewport_size().height);
    window.press(&support::shortcut("l"), cx);
    assert_eq!(
      window
        .find(harness.view.read(cx).composer_input_id())
        .focused(),
      Some(true)
    );
  })
  .expect("small native window keeps composer reachable");
}

#[gpui_kit::test]
async fn approval_click_uses_exact_tool_arguments_before_real_file_effect(cx: &mut TestAppContext) {
  let arguments = json!({"path":"approved.txt","content":"approved fixture"});
  let harness = Harness::new(
    cx,
    vec![
      tool_reply(
        StandardProtocol::ChatCompletions,
        "write_file",
        arguments.clone(),
      ),
      text_reply(
        StandardProtocol::ChatCompletions,
        "Written after your approval.",
      ),
    ],
  );
  harness.connect(cx);
  harness
    .wait(cx, |snapshot| snapshot.status.starts_with("Connected"))
    .await;
  harness.input_prompt(cx, "Write approved.txt in this workspace.");
  let approval = harness
    .wait(cx, |snapshot| !snapshot.approvals.is_empty())
    .await;
  assert_eq!(approval.approvals[0].1, "write_file");
  assert_eq!(
    serde_json::from_str::<serde_json::Value>(&approval.approvals[0].2).expect("approval args"),
    arguments
  );
  let path = harness.directory.path().join("workspace/approved.txt");
  assert!(
    !path.exists(),
    "mutating tool must wait for the native approval click"
  );
  let cursor = approval.event_cursor;
  harness.click("inspect", cx);
  harness
    .wait(cx, |state| {
      state.approvals.len() == 1 && state.event_cursor >= cursor
    })
    .await;
  harness.click("inspect", cx);
  harness.click(("allow", 0usize), cx);
  harness
    .wait(cx, |snapshot| {
      snapshot.active_run.is_none()
        && snapshot
          .messages
          .iter()
          .any(|(_, body)| body.contains("Written after"))
    })
    .await;
  assert_eq!(
    std::fs::read_to_string(path).expect("approved file effect"),
    "approved fixture"
  );
  let completed = harness.snapshot(cx);
  assert_eq!(
    completed
      .messages
      .iter()
      .filter(|(role, _)| role == "You")
      .count(),
    1,
    "resubscribing must not repeat prompts"
  );
  assert_eq!(
    completed
      .messages
      .iter()
      .filter(|(role, body)| role == "Tool" && body.contains("tool_finished"))
      .count(),
    1,
    "duplicate watch streams must deduplicate durable event cursors"
  );
  assert_eq!(harness.runtime.block_on(harness.model.requests()).len(), 2);
}

#[gpui_kit::test]
async fn stop_click_cancels_real_daemon_run_without_resending_prompt(cx: &mut TestAppContext) {
  let harness = Harness::new(cx, vec![ScriptedReply::stall()]);
  harness.connect(cx);
  harness
    .wait(cx, |snapshot| snapshot.status.starts_with("Connected"))
    .await;
  harness
    .runtime
    .block_on(harness.client.prepare_upgrade())
    .expect("private daemon enters draining state");
  harness.input_prompt(cx, "Keep this run pending until I stop it.");
  let rejected = harness
    .wait(cx, |state| state.error.is_some() && !state.busy)
    .await;
  assert_eq!(
    rejected.draft, "Keep this run pending until I stop it.",
    "draft is retained until the daemon acknowledges a run"
  );
  assert!(!rejected.messages.iter().any(|(role, _)| role == "You"));
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty(),
    "a rejected submit must not call the model"
  );
  harness
    .runtime
    .block_on(harness.client.resume_accepting())
    .expect("private daemon resumes accepting");
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("enter"), cx)
  })
  .expect("explicit retry of retained draft");
  harness
    .wait(cx, |snapshot| snapshot.active_run.is_some())
    .await;
  harness.runtime.block_on(async {
    tokio::time::timeout(
      std::time::Duration::from_secs(3),
      harness.model.wait_for_requests(1),
    )
    .await
    .expect("model request started");
  });
  harness.click("cancel", cx);
  harness
    .wait(cx, |snapshot| {
      snapshot.active_run.is_none() && snapshot.status.contains("cancelled")
    })
    .await;
  assert_eq!(harness.runtime.block_on(harness.model.requests()).len(), 1);
}

use gpui_kit::AppContext as _;
mod support;
use gpui_kit::{TestAppContext, test::TestWindowExt as _};
use support::Harness;

fn tab(harness: &Harness, index: usize, cx: &mut TestAppContext) {
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.within("tab-bar").click(index, cx)
  })
  .expect("native workbench tab click");
}
fn edit(harness: &Harness, text: &str, cx: &mut TestAppContext) {
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("workspace-editor", cx);
    window.press(&support::shortcut("a"), cx);
    window.input(text, cx);
  })
  .expect("native Editor input");
}

#[gpui_kit::test]
async fn native_editor_save_conflict_and_diff_use_real_workspace_rpc(cx: &mut TestAppContext) {
  let write_gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
  let harness = Harness::with_write_gate(cx, Vec::new(), write_gate.clone());
  let workspace = harness.directory.path().join("workspace");
  let path = workspace.join("example.rs");
  std::fs::write(&path, "fn original() {}\n").expect("tracked fixture");
  for args in [
    vec!["init", "--quiet"],
    vec!["add", "example.rs"],
    vec![
      "-c",
      "user.name=Fixture",
      "-c",
      "user.email=fixture@example.invalid",
      "commit",
      "--quiet",
      "-m",
      "fixture",
    ],
  ] {
    assert!(
      std::process::Command::new("git")
        .current_dir(&workspace)
        .args(args)
        .status()
        .expect("local Git fixture")
        .success()
    );
  }
  harness.connect(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  harness.show_workbench(cx);
  harness.click("new-session", cx);
  harness
    .wait(cx, |state| {
      state.workbench.paths == ["example.rs"] && !state.workbench.busy
    })
    .await;
  tab(&harness, 1, cx);
  harness.click(("workspace-file", 0usize), cx);
  harness
    .wait(cx, |state| {
      state.workbench.file.as_deref() == Some("example.rs") && !state.workbench.busy
    })
    .await;
  tab(&harness, 2, cx);
  // Saving unchanged text still owns a pending RPC and must block reconnect.
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("workspace-editor", cx);
    window.press(&support::shortcut("s"), cx);
  })
  .expect("save unchanged file through native keyboard shortcut");
  harness.wait(cx, |state| state.workbench.busy).await;
  let session_before_save = harness.snapshot(cx).selected_session;
  harness.click("open-settings", cx);
  harness.click("connect", cx);
  let pending = harness.snapshot(cx);
  assert_eq!(pending.selected_session, session_before_save);
  assert!(
    pending.status.starts_with("Connected"),
    "an unchanged pending save also blocks reconnect"
  );
  harness.click("close-settings", cx);
  write_gate.add_permits(1);
  harness.wait(cx, |state| !state.workbench.busy).await;
  edit(&harness, "fn saved() {}\n", cx);
  assert_eq!(harness.snapshot(cx).workbench.content, "fn saved() {}\n");
  let edited_tab = harness
    .snapshot(cx)
    .tabs
    .iter()
    .find(|tab| tab.selected)
    .expect("editor tab")
    .id;
  harness.click("new-chat-tab", cx);
  harness
    .wait(cx, |state| state.selected_session.is_some() && !state.busy)
    .await;
  harness.click(("chat-tab", edited_tab), cx);
  let restored = harness.wait(cx, |state| !state.workbench.busy).await;
  assert_eq!(restored.workbench.file.as_deref(), Some("example.rs"));
  assert_eq!(
    restored.workbench.content, "fn saved() {}\n",
    "switching chats preserves unsaved editor text and its base revision"
  );
  assert_eq!(
    std::fs::read_to_string(&path).expect("still unsaved"),
    "fn original() {}\n"
  );
  harness.click("workspace-editor", cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("s"), cx)
  })
  .expect("native Save shortcut");
  harness.wait(cx, |state| state.workbench.busy).await;
  assert_eq!(
    std::fs::read_to_string(&path).expect("write held at RPC boundary"),
    "fn original() {}\n"
  );
  let tabs_before_ack = harness.snapshot(cx).tabs.len();
  harness.click("new-chat-tab", cx);
  assert_eq!(
    harness.snapshot(cx).tabs.len(),
    tabs_before_ack,
    "an in-flight save keeps its original revision owner until acknowledgement"
  );
  assert_eq!(
    harness.snapshot(cx).selected_session,
    restored.selected_session
  );
  write_gate.close();
  harness
    .wait(cx, |state| {
      !state.workbench.busy
        && std::fs::read_to_string(&path).ok().as_deref() == Some("fn saved() {}\n")
    })
    .await;
  edit(&harness, "fn draft() {}\n", cx);
  std::fs::write(&path, "fn external() {}\n").expect("competing file writer");
  harness.click("file-save", cx);
  let conflict = harness
    .wait(cx, |state| {
      state
        .workbench
        .error
        .as_ref()
        .is_some_and(|error| error.contains("revision changed"))
    })
    .await;
  assert_eq!(
    conflict.workbench.content, "fn draft() {}\n",
    "failed save preserves native editor draft"
  );
  assert_eq!(
    std::fs::read_to_string(&path).expect("external version"),
    "fn external() {}\n"
  );
  harness.click("file-reload", cx);
  harness
    .wait(cx, |state| {
      state.workbench.content == "fn external() {}\n" && state.workbench.error.is_none()
    })
    .await;
  tab(&harness, 3, cx);
  harness.click("diff-refresh", cx);
  let diff = harness
    .wait(cx, |state| {
      state.workbench.patch.contains("+fn external()") && !state.workbench.busy
    })
    .await;
  assert!(diff.workbench.patch.contains("-fn original()"));
  cx.update_window(harness.window.into(), |_, window, cx| {
    assert!(window.find("diff-panel").visible());
    window.click("diff-mode", cx);
    assert!(window.find("diff-panel").visible());
  })
  .expect("native GPUI Kit Diff interaction");

  let readme = workspace.join("README.md");
  std::fs::write(&readme, "Original workspace overview\n").expect("tracked README fixture");
  for args in [
    vec!["add", "README.md"],
    vec![
      "-c",
      "user.name=Fixture",
      "-c",
      "user.email=fixture@example.invalid",
      "commit",
      "--quiet",
      "-m",
      "workspace overview",
    ],
  ] {
    assert!(
      std::process::Command::new("git")
        .current_dir(&workspace)
        .args(args)
        .status()
        .expect("local Git fixture")
        .success()
    );
  }
  std::fs::write(&readme, "Updated workspace overview\n").expect("README change");
  std::fs::write(workspace.join("notes.txt"), "Untracked editor notes\n")
    .expect("untracked file fixture");
  tab(&harness, 1, cx);
  harness.click("files-refresh", cx);
  let files = harness
    .wait(cx, |state| {
      state.workbench.paths.iter().any(|path| path == "notes.txt") && !state.workbench.busy
    })
    .await;
  let notes_index = files
    .workbench
    .paths
    .iter()
    .position(|path| path == "notes.txt")
    .expect("untracked file in native file list");
  cx.update_window(harness.window.into(), |_, window, _| {
    let notes = window.find(("workspace-file", notes_index));
    assert_eq!(notes.label(), Some("Open file notes.txt"));
    assert_eq!(notes.role(), Some(gpui_kit::Role::Button));
  })
  .expect("native file accessibility label");
  harness.click(("workspace-file", notes_index), cx);
  harness
    .wait(cx, |state| {
      state.workbench.file.as_deref() == Some("notes.txt") && !state.workbench.busy
    })
    .await;
  tab(&harness, 2, cx);
  assert_eq!(
    harness.snapshot(cx).workbench.content,
    "Untracked editor notes\n"
  );
  tab(&harness, 3, cx);
  harness.click("diff-refresh", cx);
  let workspace_diff = harness
    .wait(cx, |state| {
      state
        .workbench
        .patch
        .contains("+Updated workspace overview")
        && !state.workbench.busy
    })
    .await;
  assert!(
    workspace_diff
      .workbench
      .patch
      .contains("-Original workspace overview")
  );
  assert_eq!(workspace_diff.workbench.file.as_deref(), Some("notes.txt"));
  tab(&harness, 2, cx);
  let selected_before_reconnect = harness.snapshot(cx).selected_session;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("open-settings", cx);
    window.click("connect", cx);
    assert!(
      harness
        .view
        .read(cx)
        .snapshot(cx)
        .status
        .starts_with("Connecting")
    );
    window.click("close-settings", cx);
    window.click("workspace-editor", cx);
    window.input("must not overwrite during reconnect", cx);
    assert_eq!(
      harness.view.read(cx).snapshot(cx).workbench.content,
      "Untracked editor notes\n",
      "pending connection replacement must freeze the old workspace editor"
    );
  })
  .expect("reconnect keeps the previous editor read-only until authentication completes");
  harness
    .wait(cx, |state| {
      state.status.starts_with("Connected") && state.selected_session == selected_before_reconnect
    })
    .await;
  assert_eq!(
    harness.snapshot(cx).workbench.content,
    "Untracked editor notes\n"
  );
}

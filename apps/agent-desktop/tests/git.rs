mod support;
use gpui_kit::{AppContext as _, TestAppContext, test::TestWindowExt as _};
use support::Harness;

fn panel(harness: &Harness, index: usize, cx: &mut TestAppContext) {
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.render_frame(cx);
    window.click("menu", cx);
    window.within("popup-menu").click(index, cx);
  })
  .expect("workbench panel menu");
}

#[gpui_kit::test]
async fn git_status_file_patch_and_diff_are_isolated_and_cleared_when_switching_chats(
  cx: &mut TestAppContext,
) {
  let harness = Harness::new(cx, Vec::new());
  let workspace = harness.directory.path().join("workspace");
  std::fs::write(workspace.join("tracked.rs"), "fn before() {}\n").unwrap();
  std::fs::write(workspace.join("deleted.txt"), "Deleted tracked contents\n").unwrap();
  std::fs::write(workspace.join("[abc].txt"), "Original literal file\n").unwrap();
  std::fs::write(workspace.join("a.txt"), "Original sibling file\n").unwrap();
  for args in [
    vec!["init", "--quiet"],
    vec!["add", "."],
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
        .unwrap()
        .success()
    );
  }
  std::fs::write(workspace.join("tracked.rs"), "fn after() {}\n").unwrap();
  std::fs::write(workspace.join("untracked.txt"), "Untracked fixture\n").unwrap();
  std::fs::remove_file(workspace.join("deleted.txt")).unwrap();
  std::fs::write(workspace.join("[abc].txt"), "Changed literal file\n").unwrap();
  std::fs::write(workspace.join("a.txt"), "Changed sibling file\n").unwrap();
  harness.connect(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  harness.click("new-session", cx);
  let original = harness
    .wait(cx, |state| {
      state.selected_session.is_some() && !state.workbench.busy && !state.busy
    })
    .await;
  let original_tab = original.tabs.iter().find(|tab| tab.selected).unwrap().id;
  let original_session = original.selected_session.unwrap();
  harness.show_workbench(cx);
  // Existing Diff remains independent from the Git pane's parsed DiffState.
  panel(&harness, 3, cx);
  harness.click("diff-refresh", cx);
  let diff = harness
    .wait(cx, |state| {
      state.workbench.patch.contains("+fn after()") && !state.workbench.busy
    })
    .await
    .workbench
    .patch;
  panel(&harness, 6, cx);
  harness.click("git-refresh", cx);
  let status = harness
    .wait(cx, |state| {
      state.workbench.git.status.is_some() && !state.workbench.git.busy
    })
    .await
    .workbench
    .git
    .status
    .unwrap();
  assert!(status.repository);
  assert!(!status.branch.is_empty());
  assert!(
    status
      .entries
      .iter()
      .any(|entry| entry.path == "untracked.txt")
  );
  let modified = status
    .entries
    .iter()
    .position(|entry| entry.path == "tracked.rs")
    .unwrap();
  let deleted = status
    .entries
    .iter()
    .position(|entry| entry.path == "deleted.txt")
    .unwrap();
  let untracked = status
    .entries
    .iter()
    .position(|entry| entry.path == "untracked.txt")
    .unwrap();
  cx.update_window(harness.window.into(), |_, window, _| {
    assert_eq!(window.find(("git-file", untracked)).disabled(), Some(true));
    assert_ne!(window.find(("git-file", modified)).disabled(), Some(true));
    assert!(window.find(("git-untracked", untracked)).visible());
    assert_eq!(
      window.find(("git-untracked", untracked)).label(),
      Some("Untracked preview unavailable; open this file in Files")
    );
  })
  .unwrap();
  harness.click(("git-file", modified), cx);
  let patch = harness
    .wait(cx, |state| {
      !state.workbench.git.busy && state.workbench.git.path == "tracked.rs"
    })
    .await;
  assert!(patch.workbench.git.error.is_none());
  assert_eq!(patch.workbench.git.rendered_files, 1);
  assert!(patch.workbench.git.patch.contains("+fn after()"));
  assert!(!patch.workbench.git.patch.contains("deleted.txt"));
  assert_eq!(
    patch.workbench.patch, diff,
    "Git preview must not overwrite the Diff panel"
  );
  harness.click(("git-file", deleted), cx);
  let deletion = harness
    .wait(cx, |state| {
      !state.workbench.git.busy && state.workbench.git.path == "deleted.txt"
    })
    .await;
  assert!(
    deletion
      .workbench
      .git
      .patch
      .contains("-Deleted tracked contents")
  );
  assert_eq!(deletion.workbench.git.rendered_files, 1);
  let literal = status
    .entries
    .iter()
    .position(|entry| entry.path == "[abc].txt")
    .unwrap();
  harness.click(("git-file", literal), cx);
  let literal_patch = harness
    .wait(cx, |state| {
      !state.workbench.git.busy && state.workbench.git.path == "[abc].txt"
    })
    .await;
  assert_eq!(literal_patch.workbench.git.rendered_files, 1);
  assert!(
    literal_patch
      .workbench
      .git
      .patch
      .contains("+Changed literal file")
  );
  assert!(
    !literal_patch
      .workbench
      .git
      .patch
      .contains("Changed sibling file"),
    "file selections are literal Git paths"
  );
  harness.click("git-patch", cx);
  let all = harness
    .wait(cx, |state| {
      !state.workbench.git.busy && state.workbench.git.rendered_files == 4
    })
    .await;
  assert!(all.workbench.git.patch.contains("tracked.rs"));
  assert!(all.workbench.git.path.is_empty());
  harness.click("new-chat-tab", cx);
  let switched = harness
    .wait(cx, |state| {
      state
        .selected_session
        .as_deref()
        .is_some_and(|id| id != original_session)
        && !state.busy
        && !state.workbench.busy
    })
    .await;
  assert!(switched.workbench.git.status.is_none());
  assert!(switched.workbench.git.patch.is_empty());
  assert_eq!(switched.workbench.git.rendered_files, 0);
  assert!(switched.workbench.patch.is_empty());
  harness.click(("chat-tab", original_tab), cx);
  let returned = harness
    .wait(cx, |state| {
      state.selected_session.as_deref() == Some(&original_session) && !state.workbench.busy
    })
    .await;
  assert!(
    returned.workbench.git.status.is_none(),
    "old Git snapshots require a new daemon read"
  );
  assert!(returned.workbench.git.patch.is_empty());
  assert_eq!(returned.workbench.git.rendered_files, 0);
  harness.click("git-refresh", cx);
  let refreshed = harness
    .wait(cx, |state| {
      state.workbench.git.status.is_some() && !state.workbench.git.busy
    })
    .await;
  assert!(refreshed.workbench.git.error.is_none());
  assert_eq!(refreshed.workbench.git.status.unwrap().entries.len(), 5);
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty()
  );
}

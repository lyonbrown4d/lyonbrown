mod support;

use gpui_kit::{AppContext as _, TestAppContext, test::TestWindowExt as _};
use std::time::{Duration, Instant};
use support::Harness;

#[gpui_kit::test]
async fn changing_authenticated_endpoint_never_copies_the_previous_scope_draft(
  cx: &mut TestAppContext,
) {
  let harness = Harness::new(cx, Vec::new());
  harness.connect(cx);
  harness
    .wait(cx, |state| {
      state.status.starts_with("Connected") && state.local_state_loaded
    })
    .await;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("l"), cx);
    window.input("private draft for the first endpoint", cx);
    window.render_frame(cx);
  })
  .expect("edit first scope draft");
  let original = harness.configuration.settings.endpoint.clone();
  let alternative = original.replace("127.0.0.1", "localhost");
  assert_ne!(original, alternative);
  for (endpoint, expected) in [
    (alternative, ""),
    (original, "private draft for the first endpoint"),
  ] {
    cx.update_window(harness.window.into(), |_, window, cx| {
      window.click("open-settings", cx);
      window.click("endpoint", cx);
      window.press(&support::shortcut("a"), cx);
      window.input(&endpoint, cx);
      window.click("connect", cx);
      window.click("close-settings", cx);
    })
    .expect("change authenticated endpoint");
    let state = harness
      .wait(cx, |state| {
        state.status.starts_with("Connected")
          && state.local_state_loaded
          && !state.local_state_loading
      })
      .await;
    assert_eq!(state.draft, expected);
  }
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty()
  );
  harness.flush_local_state(cx);
}

#[gpui_kit::test]
async fn orphan_chat_draft_requires_explicit_binding_before_submission(cx: &mut TestAppContext) {
  let harness = Harness::new(cx, Vec::new());
  harness.connect(cx);
  harness.wait(cx, |state| state.local_state_loaded).await;
  harness.flush_local_state(cx);
  let file = std::fs::read_dir(harness.directory.path().join("client-state"))
    .unwrap()
    .map(|entry| entry.unwrap().path())
    .find(|path| path.extension().is_some_and(|ext| ext == "json"))
    .unwrap();
  let mut saved: serde_json::Value =
    serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
  saved["payload"]["tabs"][0]["session_id"] = serde_json::json!(uuid::Uuid::now_v7().to_string());
  saved["payload"]["tabs"][0]["composer"] = serde_json::json!("orphan draft stays local");
  std::fs::write(file, serde_json::to_vec(&saved).unwrap()).unwrap();
  let (window, view) = harness.recreate_view(cx);
  cx.update_window(window.into(), |_, window, cx| {
    window.click("open-settings", cx);
    window.click("connect", cx);
    window.click("close-settings", cx);
  })
  .unwrap();
  let restored = wait_ready(&harness, window, view.clone(), cx).await;
  assert_eq!(restored.draft, "orphan draft stays local");
  cx.update_window(window.into(), |_, window, cx| {
    assert!(window.find("bind-restored-chat").visible());
    window.press(&support::shortcut("enter"), cx);
  })
  .unwrap();
  let blocked = cx.update(|cx| view.read(cx).snapshot(cx));
  assert!(
    blocked
      .error
      .as_deref()
      .is_some_and(|error| error.contains("no session"))
  );
  assert!(harness.daemon.store.sessions().unwrap().is_empty());
  cx.update_window(window.into(), |_, window, cx| {
    window.click("bind-restored-chat", cx)
  })
  .unwrap();
  let deadline = Instant::now() + Duration::from_secs(10);
  loop {
    harness.client_runtime.block_on(async {
      tokio::time::sleep(Duration::from_millis(10)).await;
    });
    cx.executor().timer(Duration::from_millis(100)).await;
    let state = cx.update(|cx| view.read(cx).snapshot(cx));
    if state.selected_session.is_some() && !state.busy {
      assert_eq!(state.draft, "orphan draft stays local");
      assert!(
        harness
          .daemon
          .store
          .runs(state.selected_session.as_deref().unwrap())
          .unwrap()
          .is_empty()
      );
      break;
    }
    assert!(
      Instant::now() < deadline,
      "explicit binding did not finish: {state:?}"
    );
  }
  assert_eq!(harness.daemon.store.sessions().unwrap().len(), 1);
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty()
  );
  let completion = cx.update(|cx| view.update(cx, |desktop, _| desktop.flush_local_state()));
  harness.client_runtime.block_on(completion);
}

async fn wait_ready(
  harness: &Harness,
  window: gpui_kit::WindowHandle<gpui_kit::base::Root>,
  view: gpui_kit::Entity<agent_desktop::Desktop>,
  cx: &mut TestAppContext,
) -> agent_desktop::DesktopSnapshot {
  let deadline = Instant::now() + Duration::from_secs(10);
  loop {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
      .expect("render recreated desktop");
    let snapshot = cx.update(|cx| view.read(cx).snapshot(cx));
    if snapshot.status.starts_with("Connected")
      && snapshot.local_state_loaded
      && !snapshot.local_state_loading
    {
      return snapshot;
    }
    assert!(Instant::now() < deadline, "restore timed out: {snapshot:?}");
    harness.client_runtime.block_on(async {
      tokio::time::sleep(Duration::from_millis(10)).await;
    });
    cx.executor().timer(Duration::from_millis(100)).await;
  }
}

#[gpui_kit::test]
async fn recreated_desktop_restores_tabs_composer_and_editor_draft_without_saving(
  cx: &mut TestAppContext,
) {
  let harness = Harness::new(cx, Vec::new());
  let workspace = harness.directory.path().join("workspace");
  let file = workspace.join("restore.txt");
  std::fs::write(&file, "original file\n").expect("fixture workspace file");
  harness.connect(cx);
  harness
    .wait(cx, |state| {
      state.status.starts_with("Connected")
        && !state.local_state_loading
        && state.local_state_loaded
    })
    .await;

  harness.click("new-session", cx);
  let first_session = harness
    .wait(cx, |state| state.selected_session.is_some() && !state.busy)
    .await
    .selected_session
    .expect("created session");
  let first_tab = harness
    .snapshot(cx)
    .tabs
    .into_iter()
    .find(|tab| tab.selected)
    .expect("selected session tab")
    .id;
  harness.show_workbench(cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.within("tab-bar").click(1usize, cx);
  })
  .expect("select Files pane");
  harness
    .wait(cx, |state| {
      state
        .workbench
        .paths
        .iter()
        .any(|path| path == "restore.txt")
    })
    .await;
  harness.click(("workspace-file", 0usize), cx);
  harness
    .wait(cx, |state| {
      state.workbench.file.as_deref() == Some("restore.txt")
    })
    .await;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.within("tab-bar").click(2usize, cx);
  })
  .expect("select Editor pane");
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("workspace-editor", cx);
    window.press(&support::shortcut("a"), cx);
    window.input("unsaved editor draft", cx);
  })
  .expect("edit local file buffer");

  cx.update_window(harness.window.into(), |_, window, cx| {
    window.within("tab-bar").click(4usize, cx);
  })
  .expect("select durable Tasks pane before restart");

  harness.click("new-chat-tab", cx);
  harness.wait(cx, |state| !state.busy).await;
  let second_tab = harness
    .snapshot(cx)
    .tabs
    .into_iter()
    .find(|tab| tab.selected)
    .expect("selected composer tab")
    .id;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("l"), cx);
    window.input("unsent composer draft", cx);
  })
  .expect("edit composer draft");
  assert_eq!(harness.snapshot(cx).draft, "unsent composer draft");
  let sidebar_expected = !harness.snapshot(cx).sidebar_expanded;
  harness.click("toggle-sidebar", cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.within("tab-bar").click(4usize, cx);
    window.render_frame(cx);
    assert!(window.find("task-list").visible());
  })
  .expect("capture final UI state");
  let layout_sizes_before = harness.snapshot(cx).layout_sizes;
  assert_eq!(
    layout_sizes_before.len(),
    3,
    "sidebar, conversation and workbench widths"
  );
  harness.flush_local_state(cx);

  // A later disk version must make the restored editor an explicit conflict.
  std::fs::write(&file, "changed outside the editor\n").expect("external workspace change");
  let (window, view) = harness.recreate_view(cx);
  cx.update_window(window.into(), |_, window, cx| {
    window.click("open-settings", cx);
    window.click("connect", cx);
    window.click("close-settings", cx);
  })
  .expect("connect recreated desktop");
  let _connected = wait_ready(&harness, window, view.clone(), cx).await;
  let deadline = Instant::now() + Duration::from_secs(10);
  let restored = loop {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
      .expect("render restored layout");
    let snapshot = cx.update(|cx| view.read(cx).snapshot(cx));
    if snapshot.layout_sizes.len() == layout_sizes_before.len()
      && snapshot
        .layout_sizes
        .iter()
        .zip(&layout_sizes_before)
        .all(|(restored, expected)| (restored - expected).abs() < 2.0)
    {
      break snapshot;
    }
    assert!(
      Instant::now() < deadline,
      "layout restore timed out: {snapshot:?}"
    );
    harness.client_runtime.block_on(async {
      tokio::time::sleep(Duration::from_millis(10)).await;
    });
    cx.executor().timer(Duration::from_millis(100)).await;
  };
  assert_eq!(restored.tabs.len(), 3);
  assert!(restored.tabs.iter().any(|tab| tab.id == first_tab));
  assert!(restored.tabs.iter().any(|tab| tab.id == second_tab));
  assert_eq!(
    restored
      .tabs
      .iter()
      .find(|tab| tab.selected)
      .map(|tab| tab.id),
    Some(second_tab)
  );
  assert_eq!(
    restored
      .tabs
      .iter()
      .find(|tab| tab.id == first_tab)
      .and_then(|tab| tab.session_id.as_deref()),
    Some(first_session.as_str())
  );
  assert_eq!(restored.draft, "unsent composer draft");
  assert_eq!(restored.sidebar_expanded, sidebar_expected);
  cx.update_window(window.into(), |_, window, cx| {
    window.render_frame(cx);
    assert!(window.find("task-list").visible());
    assert!(window.find("task-refresh").visible());
    window.within("tab-bar").click(2usize, cx);
  })
  .expect("restored Tasks panel and switch to Editor panel");

  cx.update_window(window.into(), |_, window, cx| {
    window.click(("chat-tab", first_tab), cx);
  })
  .expect("switch to restored session tab");
  let deadline = Instant::now() + Duration::from_secs(10);
  let restored_editor = loop {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
      .expect("render restored editor");
    let snapshot = cx.update(|cx| view.read(cx).snapshot(cx));
    if snapshot.workbench.content == "unsaved editor draft"
      && snapshot.workbench.error.as_deref().is_some_and(|error| {
        error.to_lowercase().contains("changed") || error.to_lowercase().contains("conflict")
      })
    {
      break snapshot;
    }
    assert!(
      Instant::now() < deadline,
      "editor restore timed out: {snapshot:?}"
    );
    harness.client_runtime.block_on(async {
      tokio::time::sleep(Duration::from_millis(10)).await;
    });
    cx.executor().timer(Duration::from_millis(100)).await;
  };
  cx.update_window(window.into(), |_, window, _| {
    assert!(window.find("workspace-editor").visible());
  })
  .expect("restored editor panel is usable");
  assert_eq!(restored_editor.workbench.content, "unsaved editor draft");
  assert_eq!(
    std::fs::read_to_string(&file).expect("disk file remains readable"),
    "changed outside the editor\n"
  );

  let completion = cx
    .update_window(window.into(), |_, _, cx| {
      view.update(cx, |desktop, _| desktop.flush_local_state())
    })
    .expect("flush recreated state");
  harness.client_runtime.block_on(completion);
}

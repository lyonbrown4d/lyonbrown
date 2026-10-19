mod support;
use gpui_kit::{
  AppContext as _, TestAppContext, component::ActiveTheme as _, px, size, test::TestWindowExt as _,
};
use support::Harness;

#[gpui_kit::test]
async fn workbench_menu_reaches_clipped_panels_and_preserves_chat(cx: &mut TestAppContext) {
  let harness = Harness::new(cx, Vec::new());
  harness.connect(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  harness.click("new-session", cx);
  let session = harness
    .wait(cx, |state| state.selected_session.is_some() && !state.busy)
    .await
    .selected_session;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.press(&support::shortcut("l"), cx);
    window.input("Keep this draft while choosing a workbench panel", cx);
  })
  .expect("enter chat draft");
  harness.show_workbench(cx);
  cx.simulate_window_resize(harness.window.into(), size(px(960.), px(640.)));
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.render_frame(cx);
    let mut bar = window.within("tab-bar");
    let tabs = bar.within("tabs");
    assert!(!tabs.find(5usize).visible(), "Computer starts offscreen");
    window.click("menu", cx);
    for (index, label) in ["Browser", "Files", "Editor", "Diff", "Tasks", "Computer"]
      .into_iter()
      .enumerate()
    {
      let item = window.within("popup-menu").find(index);
      assert_eq!(item.label(), Some(label));
      assert!(item.visible());
    }
    window.within("popup-menu").click(5usize, cx);
    assert!(window.find("computer-panel").visible());
  })
  .expect("select hidden panel from the dock menu without scrolling");
  cx.update_window(harness.window.into(), |_, window, cx| {
    // The newly active panel offers the same navigation back to Browser.
    window.click("menu", cx);
    window.within("popup-menu").click(0usize, cx);
    assert!(window.try_find("computer-panel").is_none());
    assert_eq!(
      window
        .within("tab-bar")
        .within("tabs")
        .find(0usize)
        .selected(),
      Some(true)
    );
  })
  .expect("navigate back from the newly active panel");
  let state = harness.snapshot(cx);
  assert_eq!(state.selected_session, session);
  assert_eq!(
    state.draft,
    "Keep this draft while choosing a workbench panel"
  );
  assert!(!state.busy);
}

#[gpui_kit::test]
async fn settings_are_separate_and_both_panels_can_be_restored(cx: &mut TestAppContext) {
  let harness = Harness::new(cx, Vec::new());
  let initial = harness.snapshot(cx);
  assert!(initial.sidebar_expanded);
  assert!(!initial.workbench_expanded);
  assert!(!initial.settings_open);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.render_frame(cx);
    assert!(window.find("title-bar").visible());
    assert!(window.try_find("endpoint").is_none());
    assert!(window.try_find("access-token").is_none());
    window.press(&support::shortcut("l"), cx);
    window.input("A draft retained while opening settings", cx);
  })
  .expect("main chrome has no connection fields");
  harness.click("toggle-sidebar", cx);
  harness.click("toggle-workbench", cx);
  assert!(!harness.snapshot(cx).sidebar_expanded);
  assert!(harness.snapshot(cx).workbench_expanded);
  harness.click("toggle-workbench", cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.render_frame(cx);
    assert!(window.find("toggle-sidebar").visible());
    assert!(window.find("toggle-workbench").visible());
    assert!(window.find("composer").visible());
    window.click("open-settings", cx);
    assert!(window.find("endpoint").visible());
    assert_eq!(
      window.find("access-token").value(),
      None,
      "token remains masked"
    );
    let was_dark = cx.theme().is_dark();
    window.click(
      if was_dark {
        "theme-light"
      } else {
        "theme-dark"
      },
      cx,
    );
    assert_ne!(cx.theme().is_dark(), was_dark);
    window.press("escape", cx);
  })
  .expect("settings navigation and theme controls");
  let restored = harness.snapshot(cx);
  assert!(!restored.settings_open);
  assert_eq!(restored.draft, "A draft retained while opening settings");
  assert!(!restored.sidebar_expanded && !restored.workbench_expanded);
  harness.click("toggle-sidebar", cx);
  harness.click("toggle-workbench", cx);
  cx.simulate_window_resize(harness.window.into(), size(px(960.), px(640.)));
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.render_frame(cx);
    assert!(window.find("composer").visible());
    assert!(window.find("composer").bounds().bottom() <= window.viewport_size().height);
    assert!(window.find("toggle-sidebar").visible());
    assert!(window.find("toggle-workbench").visible());
  })
  .expect("both panels remain user-controlled at the minimum window size");
}

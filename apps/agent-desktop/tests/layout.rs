mod support;
use gpui_kit::{
  AppContext as _, TestAppContext, component::ActiveTheme as _, px, size, test::TestWindowExt as _,
};
use support::Harness;

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

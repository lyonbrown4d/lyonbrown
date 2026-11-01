use super::Harness;
use gpui_kit::{App, AppContext as _, TestAppContext, Window, test::TestWindowExt as _};
use std::time::{Duration, Instant};

pub fn page(harness: &Harness, index: usize, cx: &mut TestAppContext) {
  if !harness.snapshot(cx).settings_open {
    harness.click("open-settings", cx);
  }
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.render_frame(cx);
    window
      .within("settings-sidebar")
      .click(format!("0-{index}"), cx);
  })
  .expect("resource settings page");
}
pub async fn ready(
  harness: &Harness,
  cx: &mut TestAppContext,
  mut condition: impl FnMut(&Window, &App) -> bool,
) {
  let deadline = Instant::now() + Duration::from_secs(10);
  loop {
    if cx
      .update_window(harness.window.into(), |_, window, cx| {
        window.render_frame(cx);
        condition(window, cx)
      })
      .expect("resource frame")
    {
      return;
    }
    assert!(Instant::now() < deadline, "resource UI condition timed out");
    harness.client_runtime.block_on(async {
      tokio::time::sleep(Duration::from_millis(10)).await;
    });
    cx.executor().timer(Duration::from_millis(100)).await;
  }
}
// Some integration binaries use only the shared navigation and polling helpers.
#[allow(dead_code)]
pub async fn session(harness: &Harness, cx: &mut TestAppContext) -> String {
  harness.connect(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  harness.click("new-session", cx);
  harness
    .wait(cx, |state| {
      state.selected_session.is_some() && !state.busy && !state.workbench.busy
    })
    .await
    .selected_session
    .expect("selected session")
}

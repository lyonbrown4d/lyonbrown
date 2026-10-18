use super::state::Workbench;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{TestSupportExt as _, *};
impl Workbench {
  pub(super) fn tasks_view(&self, cx: &App) -> AnyElement {
    div()
      .id("task-list")
      .test_support()
      .size_full()
      .overflow_y_scroll()
      .p_4()
      .child(
        div()
          .mb_4()
          .text_xs()
          .text_color(cx.theme().muted_foreground)
          .child("Tasks continue in agentd when this window closes."),
      )
      .children(self.runs.iter().map(|run| {
        div()
          .mb_4()
          .flex()
          .flex_col()
          .gap_1()
          .child(
            div()
              .text_xs()
              .text_color(cx.theme().muted_foreground)
              .child(run.id.clone()),
          )
          .child(div().text_sm().child(run.prompt.clone()))
          .child(div().text_xs().child(run.status.clone()))
      }))
      .into_any_element()
  }
}

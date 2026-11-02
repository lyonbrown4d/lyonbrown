use crate::{
  Desktop,
  theme::{DesktopColors, READING_WIDTH},
};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{prelude::FluentBuilder as _, *};

impl Desktop {
  pub(crate) fn conversation(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    let colors = DesktopColors::get(cx);
    let empty = self.vm.conversation.is_empty() && self.vm.approvals.is_empty();
    // Pending human decisions must not move out of reach as the transcript grows.
    let approval_height = px((window.viewport_size().height.as_f32() * 0.42).clamp(180., 420.));
    let approvals = self
      .vm
      .approvals
      .iter()
      .enumerate()
      .map(|(index, approval)| self.approval_view(index, approval, window, cx))
      .collect::<Vec<_>>();
    div()
      .flex()
      .flex_col()
      .size_full()
      .min_w_0()
      .bg(colors.canvas)
      .text_color(colors.foreground)
      .text_size(px(15.))
      .child(self.chat_tabs(cx))
      .when(empty, |view| {
        view.child(
          div()
            .flex_1()
            .min_h_0()
            .id("conversation")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .justify_center()
            .px_6()
            .py_8()
            .child(
              div()
                .w_full()
                .max_w(px(READING_WIDTH))
                .mx_auto()
                .flex()
                .flex_col()
                .gap_6()
                .child(
                  div()
                    .text_size(px(28.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_center()
                    .child(format!("在 {} 中开始工作", self.workspace_name(cx))),
                )
                .when(self.client.is_none(), |column| {
                  column.child(
                    div()
                      .text_sm()
                      .text_center()
                      .text_color(colors.muted)
                      .child("在设置中连接 agentd，然后描述你的任务。"),
                  )
                })
                .child(self.composer_view(cx))
                .when_some(self.error_line(), |column, error| {
                  column.child(div().text_sm().text_color(cx.theme().danger).child(error))
                }),
            ),
        )
      })
      .when(!empty, |view| {
        view
          .child(
            div()
              .id("conversation")
              .flex_1()
              .min_h_0()
              .overflow_y_scroll()
              .px_6()
              .py_6()
              .child(
                div()
                  .w_full()
                  .max_w(px(READING_WIDTH))
                  .mx_auto()
                  .flex()
                  .flex_col()
                  .gap_6()
                  .children(
                    self
                      .vm
                      .conversation
                      .iter()
                      .enumerate()
                      .map(|(index, message)| self.message_row(index, message, window, cx)),
                  )
                  .when_some(self.error_line(), |column, error| {
                    column.child(div().text_sm().text_color(cx.theme().danger).child(error))
                  }),
              ),
          )
          .when(!self.vm.approvals.is_empty(), |view| {
            view.child(
              div().flex_shrink_0().px_6().child(
                div()
                  .id("approval-queue")
                  .w_full()
                  .max_w(px(READING_WIDTH))
                  .max_h(approval_height)
                  .mx_auto()
                  .overflow_y_scroll()
                  .children(approvals),
              ),
            )
          })
          .child(
            div().flex_shrink_0().px_6().pt_2().pb_4().child(
              div()
                .w_full()
                .max_w(px(READING_WIDTH))
                .mx_auto()
                .child(self.composer_view(cx)),
            ),
          )
      })
      .into_any_element()
  }
}

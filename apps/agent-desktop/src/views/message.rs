use crate::{Desktop, message::ChatMessage, theme::DesktopColors};
use gpui_kit::component::{Sizable, accordion::Accordion, text::TextView};
use gpui_kit::{TestSupportExt as _, prelude::FluentBuilder as _, *};

impl Desktop {
  pub(crate) fn message_row(
    &self,
    index: usize,
    message: &ChatMessage,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let colors = DesktopColors::get(cx);
    let role = message.role.as_ref();
    if role == "Tool" {
      let key: SharedString =
        format!("tool-details-{:?}-{index}", message.markdown.entity_id()).into();
      let expanded = window.use_keyed_state(key.clone(), cx, |_, _| false);
      let open = *expanded.read(cx);
      let details = Accordion::new(key)
        .small()
        .bordered(false)
        .h_auto()
        .text_color(colors.muted)
        .item(|item| {
          item
            .open(open)
            .title("工具执行详情")
            .child(TextView::new(&message.markdown).selectable(true))
        })
        .on_toggle_click(move |indices, _, cx| {
          expanded.update(cx, |open, cx| {
            *open = !indices.is_empty();
            cx.notify();
          });
        })
        .into_any_element();
      return div()
        .id(("message", index))
        .test_support()
        .w_full()
        .child(details)
        .into_any_element();
    }
    let user = role == "user" || role == "You";
    let assistant = role == "Lyonbrown" || role == "assistant";
    div()
      .id(("message", index))
      .test_support()
      .w_full()
      .flex()
      .when(user, |row| row.justify_end())
      .child(
        div()
          .id(("message-body", index))
          .test_support()
          .min_w_0()
          .flex()
          .flex_col()
          .gap_2()
          .when(user, |body| {
            body
              // Markdown needs a definite layout budget before measuring its text.
              .w(relative(0.86))
              .p_3()
              .rounded(px(16.))
              .bg(colors.prompt)
          })
          .when(!user, |body| body.w_full())
          .when(!user && !assistant, |body| {
            body.child(
              div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.muted)
                .child(message.role.clone()),
            )
          })
          .child(
            TextView::new(&message.markdown)
              .selectable(true)
              .w_full()
              .min_w_0()
              .max_w(relative(1.)),
          ),
      )
      .into_any_element()
  }
}

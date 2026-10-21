use crate::{Desktop, theme::DesktopColors};
use agent_protocol::v1::Approval;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
  ActiveTheme, Sizable,
  accordion::Accordion,
  button::{Button, ButtonVariants},
};
use gpui_kit::*;

impl Desktop {
  pub(crate) fn approval_view(
    &self,
    index: usize,
    approval: &Approval,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let colors = DesktopColors::get(cx);
    let arguments_height = px((window.viewport_size().height.as_f32() * 0.16).clamp(80., 160.));
    let details_id: SharedString = format!("approval-details-{}", approval.id).into();
    let expanded = window.use_keyed_state(details_id.clone(), cx, |_, _| false);
    let open = *expanded.read(cx);
    let allow_id = approval.id.clone();
    let deny_id = approval.id.clone();
    let arguments = approval.arguments_json.clone();
    div()
      .w_full()
      .py_4()
      .border_t_1()
      .border_color(colors.border)
      .flex()
      .flex_col()
      .gap_2()
      .child(
        div()
          .font_weight(FontWeight::MEDIUM)
          .child(format!("需要确认：{}", approval.tool)),
      )
      .child(
        Accordion::new(details_id)
          .small()
          .bordered(false)
          .h_auto()
          .item(|item| {
            item.open(open).title("查看完整工具参数").child(
              div()
                .id(("approval-arguments", index))
                .max_h(arguments_height)
                .overflow_y_scroll()
                .overflow_x_scroll()
                .p_3()
                .rounded_md()
                .bg(colors.context)
                .text_xs()
                .font_family(cx.theme().mono_font_family.clone())
                .child(approval.arguments_json.clone()),
            )
          })
          .on_toggle_click(move |indices, _, cx| {
            expanded.update(cx, |open, cx| {
              *open = !indices.is_empty();
              cx.notify();
            });
          }),
      )
      .child(
        div()
          .flex()
          .gap_2()
          .child(
            Button::new(("allow", index))
              .small()
              .primary()
              .label("允许一次")
              .on_click(
                cx.listener(move |this, _, _, cx| this.approve(allow_id.clone(), true, cx)),
              ),
          )
          .child(
            Button::new(("deny", index)).small().label("拒绝").on_click(
              cx.listener(move |this, _, _, cx| this.approve(deny_id.clone(), false, cx)),
            ),
          )
          .child(
            Button::new(("copy-approval", index))
              .small()
              .ghost()
              .icon(IconName::Copy)
              .accessibility_label("复制完整工具参数")
              .tooltip("复制完整参数")
              .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(arguments.clone()));
              }),
          ),
      )
      .into_any_element()
  }
}

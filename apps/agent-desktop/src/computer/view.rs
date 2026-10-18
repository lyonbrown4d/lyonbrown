use super::ComputerPane;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, button::Button, select::Select};
use gpui_kit::{TestSupportExt as _, *};

impl Render for ComputerPane {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let state = self.snapshot(cx);
    let blocked = state.pending || self.connection_pending;
    let can_refresh =
      state.available && state.selected_session.is_some() && state.bound_session.is_none();
    div()
      .id("computer-panel")
      .test_support()
      .size_full()
      .p_3()
      .flex()
      .flex_col()
      .gap_3()
      .child(div().text_sm().child("Computer scope for this chat"))
      .child(div().text_xs().child(format!(
        "Selected chat: {}",
        state.selected_session.as_deref().unwrap_or("None")
      )))
      .child(div().text_xs().child(if state.available {
        "Choose a window or monitor, then explicitly attach it to the selected chat."
      } else {
        "Native computer hosting is unavailable in this build or environment."
      }))
      .child(
        Button::new("computer-refresh")
          .small()
          .label("Refresh targets")
          .disabled(blocked || !can_refresh)
          .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
      )
      .child(
        Select::new(&self.select)
          .id("computer-scope")
          .placeholder("Select a window or monitor")
          .accessibility_label("Computer window or monitor scope")
          .disabled(blocked || !can_refresh || state.targets.is_empty()),
      )
      .child(
        Button::new("computer-attach")
          .small()
          .label("Attach to selected chat")
          .disabled(blocked || !can_refresh || state.selected_scope.is_none())
          .on_click(cx.listener(|this, _, _, cx| this.attach(cx))),
      )
      .child(
        Button::new("computer-detach")
          .small()
          .label("Detach")
          .disabled(blocked || state.bound_session.is_none())
          .on_click(cx.listener(|this, _, _, cx| this.detach(cx))),
      )
      .child(
        div()
          .id("computer-status")
          .test_support()
          .text_xs()
          .child(if state.pending {
            "Computer operation pending"
          } else if state.attached {
            "Computer runtime attached"
          } else if state.detach_warning {
            "Detached · native work may still be draining"
          } else if state.bound_session.is_some() {
            "Computer runtime disconnected · detach required"
          } else {
            "Computer runtime detached"
          }),
      )
      .child(div().text_xs().child(format!(
        "Bound chat: {}",
        state.bound_session.as_deref().unwrap_or("None")
      )))
      .child(div().text_xs().child(format!(
        "Bound scope: {}",
        state.bound_scope.as_deref().unwrap_or("None")
      )))
      .child(
        div()
          .id("computer-error")
          .test_support()
          .text_xs()
          .text_color(cx.theme().danger)
          .child(state.error.unwrap_or_default()),
      )
  }
}

use crate::{shell::Desktop, theme::DesktopColors};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
  Sizable,
  button::{Button, ButtonVariants},
  tab::{Tab, TabBar},
};
use gpui_kit::{TestSupportExt as _, *};

impl Desktop {
  pub(crate) fn chat_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
    let colors = DesktopColors::get(cx);
    let tabs = self.chat_tab_snapshots(cx);
    let selected = tabs.iter().position(|tab| tab.selected).unwrap_or_default();
    let ids = tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
    TabBar::new("chat-tabs")
      .small()
      .underline()
      .h(px(32.))
      .bg(colors.canvas)
      .text_color(colors.muted)
      .px_3()
      .flex_shrink_0()
      .w_full()
      .selected_index(selected)
      .max_width(px(240.))
      .children(tabs.into_iter().map(|tab| {
        let id = tab.id;
        let pending = if tab.selected {
          self.vm.busy || !self.vm.approvals.is_empty()
        } else {
          self
            .navigation
            .inactive
            .get(&id)
            .is_some_and(|state| state.vm.busy || !state.vm.approvals.is_empty())
        };
        let marker = if tab.active_run.is_some() || pending {
          " ●"
        } else {
          ""
        };
        Tab::new()
          .aria_label(format!("{}{marker}", tab.title))
          .child(
            div()
              .id(("chat-tab", id))
              .test_support()
              .max_w(px(180.))
              .min_w_0()
              .whitespace_nowrap()
              .text_ellipsis()
              .child(format!("{}{marker}", tab.title)),
          )
          .suffix(
            Button::new(("close-chat-tab", id))
              .ghost()
              .xsmall()
              .icon(IconName::X)
              .accessibility_label(format!("关闭 {}", tab.title))
              .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.close_chat_tab(id, window, cx);
              })),
          )
      }))
      .on_click(cx.listener(move |this, index: &usize, window, cx| {
        if let Some(id) = ids.get(*index) {
          this.activate_chat_tab(*id, window, cx);
        }
      }))
      .suffix(
        Button::new("new-chat-tab")
          .ghost()
          .small()
          .icon(IconName::Plus)
          .accessibility_label("新对话")
          .tooltip("新对话")
          .on_click(cx.listener(|this, _, window, cx| this.new_chat_tab(window, cx))),
      )
      .into_any_element()
  }
}

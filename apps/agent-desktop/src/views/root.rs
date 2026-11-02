//! Render root of the desktop entity: one frame's flush marker plus the global
//! action handlers that dispatch to chat tabs, routing and settings.

use crate::shell::{
  CloseChatTab, CloseSettings, Desktop, FocusComposer, NavigateBack, NavigateForward, NewChatTab,
  OpenSettings, SendPrompt, ToggleSidebar, ToggleWorkbench,
};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{TestSupportExt as _, *};

impl Render for Desktop {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    // Only a change marker reaches the persistence path; an idle frame does no
    // serialization.
    self.flush_snapshot_if_dirty(cx);
    div()
      .id("desktop")
      .test_support()
      .key_context("Desktop")
      .on_action(cx.listener(|this, _: &SendPrompt, window, cx| this.send(window, cx)))
      .on_action(cx.listener(|this, _: &FocusComposer, window, cx| {
        this.close_settings(window, cx);
      }))
      .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
        if !this.persistence_loading && !this.navigation.connecting {
          this.show_sidebar = !this.show_sidebar;
        }
        cx.notify();
      }))
      .on_action(cx.listener(|this, _: &ToggleWorkbench, _, cx| {
        if !this.persistence_loading && !this.navigation.connecting {
          this.show_workbench = !this.show_workbench;
        }
        cx.notify();
      }))
      .on_action(cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)))
      .on_action(cx.listener(|this, _: &CloseSettings, window, cx| {
        this.close_settings(window, cx);
      }))
      .on_action(cx.listener(|this, _: &NavigateBack, window, cx| {
        if this.router.back() {
          this.focus_route(window, cx);
        }
      }))
      .on_action(cx.listener(|this, _: &NavigateForward, window, cx| {
        if this.router.forward() {
          this.focus_route(window, cx);
        }
      }))
      .on_action(cx.listener(|this, _: &NewChatTab, window, cx| this.new_chat_tab(window, cx)))
      .on_action(cx.listener(|this, _: &CloseChatTab, window, cx| {
        if this.router.is_settings() {
          this.close_settings(window, cx);
        } else {
          this.close_active_chat_tab(window, cx);
        }
      }))
      .size_full()
      .flex()
      .flex_col()
      .bg(cx.theme().background)
      .text_color(cx.theme().foreground)
      .text_sm()
      .child(self.chrome(window, cx))
      .child(self.route_layout(window, cx))
  }
}

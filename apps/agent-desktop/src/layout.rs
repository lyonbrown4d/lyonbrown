//! Shared layout composition is independent of connection and daemon commands.
use crate::{Desktop, router::Route};
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::{prelude::FluentBuilder as _, *};

impl Desktop {
  pub(super) fn focus_route(&self, window: &mut Window, cx: &mut Context<Self>) {
    if self.router.is_settings() {
      self.settings_focus.focus(window, cx);
    } else {
      self.composer.read(cx).focus_handle(cx).focus(window, cx);
    }
    cx.notify();
  }

  pub(super) fn route_layout(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    match self.router.current() {
      Route::Chat => self.chat_layout(window, cx),
      Route::Settings(_) => self.settings_view(cx),
    }
  }

  fn chat_layout(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    div()
      .id("chat-layout")
      .flex_1()
      .min_h_0()
      .child(
        h_resizable("desktop-layout")
          .with_state(&self.main_layout)
          .when(self.show_sidebar, |layout| {
            layout.child(
              resizable_panel()
                .size(px(crate::theme::SIDEBAR_WIDTH))
                .size_range(px(240.)..px(380.))
                .child(self.sidebar(cx)),
            )
          })
          .when(!self.show_sidebar, |layout| {
            layout.child(
              resizable_panel()
                .size(px(crate::theme::RAIL_WIDTH))
                .size_range(px(crate::theme::RAIL_WIDTH)..px(crate::theme::RAIL_WIDTH))
                .child(self.sidebar_rail(cx)),
            )
          })
          .child(
            resizable_panel()
              .size_range(px(360.)..px(2000.))
              .child(self.conversation(window, cx)),
          )
          .when(self.show_workbench, |layout| {
            layout.child(
              resizable_panel()
                .size(px(460.))
                .size_range(px(260.)..px(1600.))
                .child(self.dock.clone()),
            )
          }),
      )
      .into_any_element()
  }
}

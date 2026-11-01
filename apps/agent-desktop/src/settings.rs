//! Settings share the window's route context and keep return controls fixed.
mod controls;

use crate::{
  Desktop,
  resources::ResourceSection,
  router::{Route, SettingsSection},
};
use gpui_kit::component::{
  ActiveTheme,
  button::{Button, ButtonVariants},
  resizable::{h_resizable, resizable_panel},
  sidebar::{Sidebar, SidebarMenu, SidebarMenuItem},
};
use gpui_kit::{TestSupportExt as _, *};

impl Desktop {
  pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if !self.router.is_settings() {
      self
        .router
        .navigate(Route::Settings(SettingsSection::Connection));
    }
    self.settings_focus.focus(window, cx);
    cx.notify();
  }

  pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.router.return_to_chat();
    self.composer.read(cx).focus_handle(cx).focus(window, cx);
    cx.notify();
  }

  pub(super) fn settings_view(&self, cx: &mut Context<Self>) -> AnyElement {
    let Route::Settings(section) = self.router.current() else {
      return div().into_any_element();
    };
    let sidebar = Sidebar::new("settings-menu")
      .w_full()
      .collapsible(false)
      .collapsed(false)
      .child(
        SidebarMenu::new().children(SettingsSection::ALL.map(|candidate| {
          SidebarMenuItem::new(candidate.title())
            .active(candidate == section)
            .on_click(cx.listener(move |desktop, _, window, cx| {
              desktop.router.navigate(Route::Settings(candidate));
              desktop.settings_focus.focus(window, cx);
              cx.notify();
            }))
        })),
      );
    let content = match section {
      SettingsSection::Connection => self.connection_settings(cx),
      SettingsSection::Appearance => self.appearance_settings(cx),
      SettingsSection::Models => self.resources.view(ResourceSection::Models),
      SettingsSection::Providers => self.resources.view(ResourceSection::Providers),
      SettingsSection::Skills => self.resources.view(ResourceSection::Skills),
      SettingsSection::Status => self.resources.view(ResourceSection::Status),
    };
    div()
      .id("settings-view")
      .test_support()
      .track_focus(&self.settings_focus)
      .key_context("DesktopSettings")
      .flex_1()
      .min_h_0()
      .flex()
      .flex_col()
      .child(
        div()
          .id("settings-navigation")
          .flex()
          .flex_shrink_0()
          .items_center()
          .gap_4()
          .px_6()
          .py_3()
          .border_b_1()
          .border_color(cx.theme().border)
          .child(
            Button::new("close-settings")
              .ghost()
              .label("Back to chats")
              .accessibility_label("Return to chats")
              .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx))),
          )
          .child(
            div()
              .text_lg()
              .font_weight(FontWeight::SEMIBOLD)
              .child("Settings"),
          ),
      )
      .child(
        div().id("settings-content").flex_1().min_h_0().child(
          h_resizable("desktop-settings-pages")
            .child(
              resizable_panel()
                .size(px(232.))
                .size_range(px(190.)..px(300.))
                .child(
                  div()
                    .id("settings-sidebar")
                    .test_support()
                    .size_full()
                    .child(sidebar),
                ),
            )
            .child(
              resizable_panel().child(
                div()
                  .id("settings-page-scroll")
                  .test_support()
                  .size_full()
                  .overflow_y_scroll()
                  .p_6()
                  .flex()
                  .flex_col()
                  .gap_5()
                  .child(
                    div()
                      .text_xl()
                      .font_weight(FontWeight::SEMIBOLD)
                      .child(section.title()),
                  )
                  .child(content),
              ),
            ),
        ),
      )
      .into_any_element()
  }
}

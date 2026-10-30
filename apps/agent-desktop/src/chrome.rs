use crate::{
  shell::Desktop,
  theme::{DesktopColors, SIDEBAR_WIDTH},
};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
  Selectable, Sizable, TitleBar,
  button::{Button, ButtonVariants},
};
use gpui_kit::{prelude::FluentBuilder as _, *};

impl Desktop {
  pub(super) fn chrome(&self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    let colors = DesktopColors::get(cx);
    let title = if self.show_settings {
      "设置".to_string()
    } else {
      self
        .vm
        .sessions
        .iter()
        .find(|session| Some(&session.id) == self.vm.selected.as_ref())
        .map(|session| session.title.clone())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Lyonbrown".into())
    };
    let sidebar_space = if self.show_sidebar && !self.show_settings {
      SIDEBAR_WIDTH - if cfg!(target_os = "macos") { 80. } else { 12. }
    } else {
      40.
    };
    TitleBar::new()
      .h(px(40.))
      .bg(if self.show_sidebar && !self.show_settings {
        colors.sidebar
      } else {
        colors.canvas
      })
      .border_color(colors.border)
      .child(
        div()
          .flex()
          .items_center()
          .h_full()
          .w(px(sidebar_space))
          .flex_shrink_0()
          .when(!self.show_settings, |row| {
            row.child(
              Button::new("toggle-sidebar")
                .ghost()
                .small()
                .icon(IconName::PanelLeft)
                .accessibility_label("切换会话侧栏")
                .tooltip("会话侧栏")
                .selected(self.show_sidebar)
                .toggled(self.show_sidebar)
                .on_click(cx.listener(|this, _, _, cx| {
                  if !this.persistence_loading && !this.navigation.connecting {
                    this.show_sidebar = !this.show_sidebar;
                  }
                  cx.notify();
                })),
            )
          }),
      )
      .child(
        div()
          .flex_1()
          .min_w_0()
          .h_full()
          .bg(colors.canvas)
          .flex()
          .items_center()
          .px_4()
          .child(
            div()
              .min_w_0()
              .truncate()
              .text_size(px(13.))
              .font_weight(FontWeight::MEDIUM)
              .text_color(colors.foreground)
              .child(title),
          )
          .child(div().flex_1())
          .when(!self.show_settings, |row| {
            row.child(
              Button::new("toggle-workbench")
                .ghost()
                .small()
                .icon(IconName::PanelRight)
                .accessibility_label("切换文件与上下文工作区")
                .tooltip("文件与上下文")
                .selected(self.show_workbench)
                .toggled(self.show_workbench)
                .on_click(cx.listener(|this, _, _, cx| {
                  if !this.persistence_loading && !this.navigation.connecting {
                    this.show_workbench = !this.show_workbench;
                  }
                  cx.notify();
                })),
            )
          })
          .child(
            Button::new("open-settings")
              .ghost()
              .small()
              .icon(IconName::Settings)
              .accessibility_label("设置")
              .tooltip("设置")
              .selected(self.show_settings)
              .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
          ),
      )
      .into_any_element()
  }
}

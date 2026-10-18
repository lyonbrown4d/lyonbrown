use crate::shell::Desktop;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
  ActiveTheme, Selectable, Sizable, TitleBar,
  button::{Button, ButtonVariants},
};
use gpui_kit::{prelude::FluentBuilder as _, *};

impl Desktop {
  pub(super) fn chrome(&self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    let title = if self.show_settings {
      "Settings".to_string()
    } else {
      self
        .sessions
        .iter()
        .find(|session| Some(&session.id) == self.selected.as_ref())
        .map(|session| session.title.chars().take(48).collect::<String>())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Lyonbrown".into())
    };
    TitleBar::new()
      .bg(cx.theme().background)
      .border_color(cx.theme().border)
      .child(
        div()
          .flex()
          .items_center()
          .w(px(80.))
          .gap_1()
          .when(!self.show_settings, |this| {
            this.child(
              Button::new("toggle-sidebar")
                .ghost()
                .small()
                .icon(IconName::PanelLeft)
                .accessibility_label("Toggle sidebar")
                .tooltip("Toggle sidebar")
                .selected(self.show_sidebar)
                .toggled(self.show_sidebar)
                .on_click(cx.listener(|this, _, _, cx| {
                  this.show_sidebar = !this.show_sidebar;
                  cx.notify();
                })),
            )
          }),
      )
      .child(
        div().flex_1().min_w_0().flex().justify_center().child(
          div()
            .max_w(px(360.))
            .truncate()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(cx.theme().muted_foreground)
            .child(title),
        ),
      )
      .child(
        div()
          .flex()
          .items_center()
          .justify_end()
          .w(px(80.))
          .gap_1()
          .pr_2()
          .when(!self.show_settings, |this| {
            this.child(
              Button::new("toggle-workbench")
                .ghost()
                .small()
                .icon(IconName::PanelRight)
                .accessibility_label("Toggle workbench")
                .tooltip("Toggle workbench")
                .selected(self.show_workbench)
                .toggled(self.show_workbench)
                .on_click(cx.listener(|this, _, _, cx| {
                  this.show_workbench = !this.show_workbench;
                  cx.notify();
                })),
            )
          })
          .child(
            Button::new("open-settings")
              .ghost()
              .small()
              .icon(IconName::Settings)
              .accessibility_label("Open settings")
              .tooltip("Settings")
              .selected(self.show_settings)
              .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
          ),
      )
      .into_any_element()
  }
}

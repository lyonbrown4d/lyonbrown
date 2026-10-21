//! Connection and appearance live in a dedicated page, outside the chat chrome.
use crate::Desktop;
use gpui_kit::component::{
  ActiveTheme, Disableable, Selectable, Theme, ThemeMode,
  button::{Button, ButtonVariants},
  input::Input,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

impl Desktop {
  pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.show_settings = true;
    self.endpoint.read(cx).focus_handle(cx).focus(window, cx);
    cx.notify();
  }

  pub(super) fn settings_view(&self, cx: &mut Context<Self>) -> AnyElement {
    div()
      .id("settings-view")
      .key_context("DesktopSettings")
      .flex_1()
      .min_h_0()
      .overflow_y_scroll()
      .child(
        div()
          .max_w(px(720.))
          .mx_auto()
          .p_8()
          .flex()
          .flex_col()
          .gap_6()
          .child(
            div()
              .flex()
              .items_center()
              .gap_4()
              .child(
                Button::new("close-settings")
                  .ghost()
                  .label("Back to chats")
                  .on_click(cx.listener(|this, _, window, cx| {
                    this.show_settings = false;
                    this.composer.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                  })),
              )
              .child(
                div()
                  .text_2xl()
                  .font_weight(FontWeight::SEMIBOLD)
                  .child("Settings"),
              ),
          )
          .child(section(
            "Connection",
            "Connect this desktop to your agent daemon.",
            cx,
          ))
          .child(field(
            "Daemon address",
            Input::new(&self.endpoint).id("endpoint").into_any_element(),
            cx,
          ))
          .child(field(
            "Access token",
            Input::new(&self.token)
              .id("access-token")
              .into_any_element(),
            cx,
          ))
          .child(
            div()
              .flex()
              .items_center()
              .gap_3()
              .child(
                Button::new("connect")
                  .primary()
                  .label(if self.navigation.connecting {
                    "Connecting…"
                  } else {
                    "Connect"
                  })
                  .disabled(
                    self.any_active_work()
                      || self.tasks.read(cx).blocks_connection(cx)
                      || self.tasks.read(cx).pending_browser_operation
                      || self.tasks.read(cx).computer.read(cx).pending,
                  )
                  .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
              )
              .child(
                div()
                  .text_sm()
                  .text_color(cx.theme().muted_foreground)
                  .child(self.vm.status.clone()),
              ),
          )
          .when(
            self.any_active_work()
              || self.tasks.read(cx).blocks_connection(cx)
              || self.tasks.read(cx).pending_browser_operation
              || self.tasks.read(cx).computer.read(cx).pending,
            |this| {
              this.child(
                div()
                  .text_sm()
                  .text_color(cx.theme().muted_foreground)
                  .child("Finish active work and save file edits before changing the connection."),
              )
            },
          )
          .when_some(self.vm.error.clone(), |this, error| {
            this.child(
              div()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().danger)
                .child(error),
            )
          })
          .child(section(
            "Workspace",
            "Choose the daemon-side folder for new chats in this tab.",
            cx,
          ))
          .child(field(
            "Workspace path",
            Input::new(&self.workspace)
              .id("workspace")
              .into_any_element(),
            cx,
          ))
          .child(section(
            "Appearance",
            "Choose a light or dark appearance for the desktop.",
            cx,
          ))
          .child(
            div()
              .flex()
              .gap_2()
              .child(
                Button::new("theme-light")
                  .label("Light")
                  .selected(!cx.theme().mode.is_dark())
                  .on_click(|_, window, cx| Theme::change(ThemeMode::Light, Some(window), cx)),
              )
              .child(
                Button::new("theme-dark")
                  .label("Dark")
                  .selected(cx.theme().mode.is_dark())
                  .on_click(|_, window, cx| Theme::change(ThemeMode::Dark, Some(window), cx)),
              ),
          ),
      )
      .into_any_element()
  }
}

fn section(title: &'static str, description: &'static str, cx: &App) -> AnyElement {
  div()
    .flex()
    .flex_col()
    .gap_2()
    .pt_3()
    .child(div().text_lg().font_weight(FontWeight::MEDIUM).child(title))
    .child(
      div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(description),
    )
    .into_any_element()
}

fn field(title: &'static str, input: AnyElement, cx: &App) -> AnyElement {
  div()
    .flex()
    .flex_col()
    .gap_2()
    .child(
      div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(title),
    )
    .child(input)
    .into_any_element()
}

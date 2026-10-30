use crate::{Desktop, theme::DesktopColors};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
  Disableable, Sizable,
  button::{Button, ButtonVariants},
  input::Textarea,
};
use gpui_kit::{TestSupportExt as _, prelude::FluentBuilder as _, *};

impl Desktop {
  pub(crate) fn connection_label(&self) -> &'static str {
    if self.persistence_loading {
      "恢复本地状态…"
    } else if self.navigation.connecting {
      "连接中…"
    } else if self.vm.status == "Run outcome unknown · inspect daemon"
      || self.vm.status == "Connection or request failed"
    {
      "需要检查连接"
    } else if self.client.is_some() {
      "agentd · 已连接"
    } else {
      "连接 agentd"
    }
  }

  pub(crate) fn workspace_name(&self, cx: &App) -> String {
    std::path::Path::new(self.workspace.read(cx).value().as_ref())
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      .filter(|name| !name.is_empty())
      .unwrap_or_else(|| "工作区".into())
  }

  pub(crate) fn composer_view(&self, cx: &mut Context<Self>) -> AnyElement {
    let colors = DesktopColors::get(cx);
    let running = self.vm.active_run.is_some();
    div()
      .w_full()
      .min_w_0()
      .flex()
      .flex_col()
      .when(
        self
          .navigation
          .unbound
          .contains_key(&self.navigation.active),
        |view| {
          view.child(
            div()
              .p_2()
              .flex()
              .flex_col()
              .gap_2()
              .child("原会话当前不可用。草稿尚未绑定，恢复不会发送内容。")
              .child(
                Button::new("bind-restored-chat")
                  .small()
                  .label("为此草稿新建会话")
                  .disabled(
                    self.client.is_none()
                      || self.navigation.connecting
                      || self.persistence_loading
                      || self.vm.busy,
                  )
                  .on_click(cx.listener(|desktop, _, _, cx| desktop.create_session(cx))),
              ),
          )
        },
      )
      .child(
        div()
          .bg(colors.composer)
          .border_1()
          .border_color(colors.border)
          .rounded(px(20.))
          .p_3()
          .flex()
          .flex_col()
          .gap_2()
          .child(
            div()
              .id("composer")
              .test_support()
              .key_context("Composer")
              .on_action(cx.listener(
                |this, action: &gpui_kit::component::input::Enter, window, cx| {
                  if !action.shift {
                    this.send(window, cx);
                  }
                },
              ))
              .child(
                Textarea::new(&self.composer)
                  .accessibility_id("composer")
                  .aria_label("输入任务或补充要求")
                  .appearance(false)
                  .bordered(false)
                  .readonly(self.vm.busy || self.navigation.connecting || self.persistence_loading)
                  .text_size(px(15.))
                  .h(px(66.)),
              ),
          )
          .child(
            div()
              .flex()
              .items_center()
              .gap_2()
              .child(
                Button::new("composer-context")
                  .ghost()
                  .small()
                  .icon(IconName::PanelRight)
                  .accessibility_label("打开文件与上下文工作区")
                  .tooltip("文件与上下文")
                  .on_click(cx.listener(|this, _, _, cx| {
                    if !this.persistence_loading && !this.navigation.connecting {
                      this.show_workbench = !this.show_workbench;
                    }
                    cx.notify();
                  })),
              )
              .when(!self.vm.approvals.is_empty(), |row| {
                row.child(div().text_xs().text_color(colors.muted).child("等待确认"))
              })
              .child(div().flex_1())
              .when(running, |row| {
                row
                  .child(
                    Button::new("inspect")
                      .ghost()
                      .small()
                      .label(if self.vm.error.is_some() {
                        "检查 / 重连"
                      } else {
                        "刷新任务"
                      })
                      .on_click(cx.listener(|this, _, _, cx| this.inspect_run(cx))),
                  )
                  .child(
                    Button::new("cancel")
                      .ghost()
                      .small()
                      .icon(IconName::Square)
                      .label("停止")
                      .accessibility_label("停止当前任务")
                      .on_click(cx.listener(|this, _, _, cx| this.cancel_run(cx))),
                  )
              })
              .when(!running, |row| {
                row.child(
                  Button::new("send")
                    .primary()
                    .icon(IconName::ArrowUp)
                    .rounded(px(18.))
                    .size(px(34.))
                    .accessibility_label("发送")
                    .tooltip(if cfg!(target_os = "macos") {
                      "Enter 发送 · Shift+Enter 换行 · ⌘ Enter 发送"
                    } else {
                      "Enter 发送 · Shift+Enter 换行 · Ctrl Enter 发送"
                    })
                    .disabled(self.client.is_none() || self.navigation.connecting || self.vm.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                )
              }),
          ),
      )
      .child(
        div()
          .mx_1()
          .px_2()
          .h(px(40.))
          .rounded_b(px(16.))
          .bg(colors.context)
          .flex()
          .items_center()
          .gap_2()
          .child(
            Button::new("composer-workspace")
              .ghost()
              .small()
              .icon(IconName::Folder)
              .label(self.workspace_name(cx))
              .flex_1()
              .min_w_0()
              .max_w(px(240.))
              .accessibility_label("工作区设置")
              .tooltip("在设置中选择工作区")
              .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
          )
          .child(div().h_4().border_l_1().border_color(colors.border))
          .child(
            Button::new("composer-connection")
              .ghost()
              .small()
              .icon(IconName::Monitor)
              .flex_1()
              .min_w_0()
              .label(self.connection_label())
              .accessibility_label("连接设置")
              .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
          ),
      )
      .into_any_element()
  }
}

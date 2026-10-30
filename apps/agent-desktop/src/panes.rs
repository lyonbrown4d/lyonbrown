use crate::{
  shell::Desktop,
  theme::{DesktopColors, RAIL_WIDTH},
};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
  Disableable, Icon, Sizable,
  button::{Button, ButtonVariants},
};
use gpui_kit::{TestSupportExt as _, prelude::FluentBuilder as _, *};

impl Desktop {
  pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
    let colors = DesktopColors::get(cx);
    let groups = self.vm.session_groups();
    let mut rows = Vec::new();
    let mut row_index: usize = 0;
    for (workspace, sessions) in groups {
      let name = std::path::Path::new(workspace)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "工作区".into());
      rows.push(
        div()
          .mt_3()
          .mb_1()
          .px_2()
          .h_8()
          .flex()
          .items_center()
          .gap_2()
          .child(Icon::new(IconName::Folder).small())
          .child(div().min_w_0().truncate().child(name))
          .into_any_element(),
      );
      for session in sessions {
        let id = session.id.clone();
        let selected = self.vm.selected.as_deref() == Some(session.id.as_str());
        rows.push(
          Button::new(("session-button", row_index))
            .ghost()
            .small()
            .w_full()
            .h(px(34.))
            .when(selected, |row| row.bg(colors.selection))
            .accessibility_label(session.title.clone())
            .tooltip(session.title.clone())
            .child(
              div()
                .id(("session", row_index))
                .test_support()
                .w_full()
                .min_w_0()
                .pl_3()
                .flex()
                .items_center()
                .gap_2()
                .child(Icon::new(IconName::MessageSquare).small())
                .child(
                  div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(session.title.clone()),
                ),
            )
            .on_click(
              cx.listener(move |this, _, window, cx| this.select_session(id.clone(), window, cx)),
            )
            .into_any_element(),
        );
        row_index += 1;
      }
    }
    div()
      .id("sidebar")
      .test_support()
      .flex()
      .flex_col()
      .size_full()
      .px_3()
      .pt_2()
      .pb_3()
      .gap_1()
      .bg(colors.sidebar)
      .text_color(colors.foreground)
      .text_size(px(13.))
      .child(
        Button::new("new-session")
          .ghost()
          .small()
          .w_full()
          .h(px(34.))
          .disabled(self.client.is_none() || self.navigation.connecting || self.vm.busy)
          .accessibility_label("新对话")
          .child(
            div()
              .w_full()
              .flex()
              .items_center()
              .gap_2()
              .child(Icon::new(IconName::SquarePen).small())
              .child("新对话"),
          )
          .on_click(cx.listener(|this, _, window, cx| this.new_chat_tab(window, cx))),
      )
      .child(
        div()
          .mt_6()
          .px_2()
          .text_xs()
          .text_color(colors.muted)
          .child("工作区"),
      )
      .child(
        div()
          .id("sessions")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .children(rows)
          .when(self.vm.sessions.is_empty(), |list| {
            list.child(
              div()
                .p_2()
                .text_color(colors.muted)
                .child(if self.client.is_some() {
                  "还没有对话。开始一个新任务。"
                } else {
                  "连接后显示会话。"
                }),
            )
          }),
      )
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .gap_2()
          .child(
            Button::new("sidebar-settings")
              .ghost()
              .small()
              .icon(IconName::Settings)
              .label("设置")
              .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
          )
          .child(
            div()
              .text_xs()
              .text_color(colors.muted)
              .child(self.connection_label()),
          ),
      )
      .into_any_element()
  }

  pub(super) fn sidebar_rail(&self, cx: &mut Context<Self>) -> AnyElement {
    let colors = DesktopColors::get(cx);
    div()
      .id("sidebar-rail")
      .test_support()
      .w(px(RAIL_WIDTH))
      .h_full()
      .flex_shrink_0()
      .flex()
      .flex_col()
      .items_center()
      .py_2()
      .gap_2()
      .bg(colors.sidebar)
      .text_color(colors.foreground)
      .child(
        Button::new("rail-new-session")
          .ghost()
          .small()
          .icon(IconName::SquarePen)
          .accessibility_label("新对话")
          .tooltip("新对话")
          .disabled(self.client.is_none() || self.navigation.connecting || self.vm.busy)
          .on_click(cx.listener(|this, _, window, cx| this.new_chat_tab(window, cx))),
      )
      .child(
        Button::new("rail-sessions")
          .ghost()
          .small()
          .icon(IconName::MessageSquare)
          .accessibility_label("展开会话侧栏")
          .tooltip("会话")
          .on_click(cx.listener(|this, _, _, cx| {
            if !this.persistence_loading && !this.navigation.connecting {
              this.show_sidebar = true;
            }
            cx.notify();
          })),
      )
      .child(div().flex_1())
      .child(
        Button::new("rail-settings")
          .ghost()
          .small()
          .icon(IconName::Settings)
          .accessibility_label("设置")
          .tooltip("设置")
          .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
      )
      .into_any_element()
  }
}

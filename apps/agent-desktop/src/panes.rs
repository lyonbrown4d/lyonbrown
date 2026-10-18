use crate::shell::Desktop;
use gpui_kit::component::{
  ActiveTheme, Disableable, Sizable,
  button::{Button, ButtonVariants},
  input::Textarea,
  text::TextView,
};
use gpui_kit::{TestSupportExt as _, prelude::FluentBuilder as _, *};

impl Desktop {
  pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
    div()
      .id("sidebar")
      .test_support()
      .flex()
      .flex_col()
      .size_full()
      .p_4()
      .gap_3()
      .bg(cx.theme().secondary)
      .child(
        div()
          .text_lg()
          .font_weight(FontWeight::SEMIBOLD)
          .child("Lyonbrown"),
      )
      .child(
        div()
          .text_xs()
          .text_color(cx.theme().muted_foreground)
          .child("YOUR WORKSPACE"),
      )
      .child(
        div().text_sm().overflow_hidden().child(
          std::path::Path::new(self.workspace.read(cx).value().as_ref())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Workspace".into()),
        ),
      )
      .child(
        Button::new("new-session")
          .label("＋ New chat")
          .w_full()
          .disabled(self.client.is_none() || self.busy)
          .on_click(cx.listener(|this, _, window, cx| this.new_chat_tab(window, cx))),
      )
      .child(
        div()
          .mt_4()
          .text_xs()
          .text_color(cx.theme().muted_foreground)
          .child("CHATS"),
      )
      .child(
        div()
          .id("sessions")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .children(
            self
              .sessions
              .iter()
              .enumerate()
              .map(|(index, session)| {
                let id = session.id.clone();
                div()
                  .id(("session", index))
                  .test_support()
                  .px_3()
                  .py_2()
                  .rounded_md()
                  .cursor_pointer()
                  .when(
                    self.selected.as_deref() == Some(session.id.as_str()),
                    |this| this.bg(cx.theme().background),
                  )
                  .hover(|this| this.bg(cx.theme().muted))
                  .child(session.title.clone())
                  .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_session(id.clone(), window, cx)
                  }))
              })
              .collect::<Vec<_>>(),
          )
          .when(self.sessions.is_empty(), |this| {
            this.child(
              div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Connect to load your chats."),
            )
          }),
      )
      .child(
        Button::new("sidebar-settings")
          .ghost()
          .small()
          .label("Settings")
          .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
      )
      .into_any_element()
  }

  pub(super) fn conversation(&self, cx: &mut Context<Self>) -> AnyElement {
    let title = self
      .sessions
      .iter()
      .find(|session| Some(&session.id) == self.selected.as_ref())
      .map_or("New conversation", |session| session.title.as_str())
      .to_string();
    div().flex().flex_col().size_full().min_w_0()
            .child(div().h(px(54.)).px_6().flex().items_center().border_b_1().border_color(cx.theme().border)
                .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("agentd")))
            .child(div().id("conversation").flex_1().min_h_0().overflow_y_scroll().p_6().children(
                self.conversation.iter().map(|message| div().mb_6().flex().flex_col().gap_2()
                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(message.role.clone()))
                    .child(TextView::new(&message.markdown).selectable(true))).collect::<Vec<_>>()
            ).when(self.conversation.is_empty(), |this| this.child(div().flex().flex_col().items_center().justify_center().h_full().gap_3()
                .child(div().text_2xl().font_weight(FontWeight::MEDIUM).child("What shall we build?"))
                .child(div().text_sm().text_center().text_color(cx.theme().muted_foreground)
                    .child("Connect to your daemon, then start a chat.\nYour sessions and jobs stay with agentd.")))))
            .when_some(self.error.clone(), |this, error| this.child(div().mx_5().p_3().rounded_md().border_1().border_color(cx.theme().danger)
                .text_sm().child(error)))
            .children(self.approvals.iter().enumerate().map(|(index, approval)| {
                let allow_id = approval.id.clone();
                let deny_id = approval.id.clone();
                div().mx_5().p_3().rounded_md().border_1().border_color(cx.theme().border).flex().flex_col().gap_2()
                    .child(format!("Permission required: {}", approval.tool))
                    .child(div().text_xs().child(approval.arguments_json.clone()))
                    .child(div().flex().gap_2()
                        .child(Button::new(("allow", index)).label("Allow once").primary()
                            .on_click(cx.listener(move |this, _, _, cx| this.approve(allow_id.clone(), true, cx))))
                        .child(Button::new(("deny", index)).label("Deny")
                            .on_click(cx.listener(move |this, _, _, cx| this.approve(deny_id.clone(), false, cx)))))
            }).collect::<Vec<_>>())
            .child(div().p_5().flex().flex_col().gap_3()
                .child(div().id("composer").test_support().key_context("Composer").child(Textarea::new(&self.composer).accessibility_id("composer").readonly(self.busy).h(px(110.))))
                .child(div().flex().items_center().gap_2()
                    .child(div().flex_1().text_xs().text_color(cx.theme().muted_foreground).child("Runs execute on your daemon"))
                    .when(self.active_run.is_some(), |this| this.child(
                        Button::new("inspect").label(if self.error.is_some(){"Inspect / reconnect"}else{"Refresh run"})
                            .on_click(cx.listener(|this, _, _, cx| this.inspect_run(cx)))))
                    .child(Button::new("cancel").ghost().label("Stop").disabled(self.active_run.is_none())
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_run(cx))))
                    .child(Button::new("send").primary().label("Send ↑").disabled(self.client.is_none() || self.busy || self.active_run.is_some())
                        .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))))).into_any_element()
  }
}

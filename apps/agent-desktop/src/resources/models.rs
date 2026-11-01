use super::state::ResourcePane;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, button::Button};
use gpui_kit::{TestSupportExt as _, *};

impl ResourcePane {
  pub(super) fn models_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    div().flex().flex_col().gap_3()
      .child("Available models")
      .child(div().id("model-selection-guidance").test_support()
        .child(if self.session.is_none() { "Select a session to use a model in a conversation." } else { "Choose an execution for the selected session." }))
      .child("Built-in, configured and installed models share the same selection list. Installing an artifact does not establish inference compatibility; load it before use.")
      .children(self.models.iter().enumerate().map(|(index, model)| {
        let model_id = model.id.clone();
        div().id(("model-candidate", index)).test_support().aria_label(model.id.clone()).p_3().border_1().border_color(cx.theme().border).rounded_md()
          .child(format!("{}{} · {} · {} · {}", if model.selected && self.session.is_some() { "Selected · " } else { "" }, model.display_name, model.vendor, model.source, model.state))
          .child(div().text_xs().child(model.id.clone()))
          .children(model.executions.iter().enumerate().map(|(execution_index, execution)| {
            let model_id = model_id.clone();
            let execution_id = execution.id.clone();
            div().flex().items_center().gap_2().mt_2()
              .child(div().flex_1().child(format!("{} · {} · {} {}", execution.label, execution.transport, execution.state, execution.reason)))
              .child(Button::new(("model-select", index * 1000 + execution_index)).small().label("Use in session")
                .disabled(self.busy || self.pending_mutations > 0 || self.session.is_none() || execution.state != "ready")
                .on_click(cx.listener(move |this, _, _, cx| this.select_model(model_id.clone(), execution_id.clone(), cx))))
          }))
      }))
      .child(self.hub_view(window, cx))
      .into_any_element()
  }
}
impl Render for ResourcePane {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let content = match self.section {
      super::ResourceSection::Models => self.models_view(window, cx),
      super::ResourceSection::Providers => self.providers_view(cx),
      super::ResourceSection::Skills => self.skills_view(window, cx),
      super::ResourceSection::Status => self.status_view(cx),
    };
    div()
      .id("resource-panel")
      .test_support()
      .w_full()
      .h(px(520.))
      .flex()
      .flex_col()
      .gap_2()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(
            Button::new("resource-refresh")
              .small()
              .label(if self.busy { "Working…" } else { "Refresh" })
              .disabled(self.busy || !self.ready() || self.pending_mutations > 0)
              .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
          )
          .child(if self.ready() {
            "Changes run on the authenticated daemon"
          } else {
            "Connect and select a session to manage resources"
          }),
      )
      .child(
        div()
          .id("resource-error")
          .test_support()
          .text_color(cx.theme().danger)
          .child(self.error.clone().unwrap_or_default()),
      )
      .child(
        div()
          .id("resource-notice")
          .test_support()
          .child(self.notice.clone().unwrap_or_default()),
      )
      .child(
        div()
          .id("resource-content")
          .test_support()
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .child(content),
      )
  }
}

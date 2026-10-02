use gpui::{Context, div, prelude::*};

use crate::{
  model_setup::{ModelSetupControl, ModelSetupIntent},
  theme::{APPROVAL_AMBER, DEEP_PANEL, SIGNAL_CYAN, color},
};

use super::Cockpit;

impl Cockpit {
  fn dispatch_model_setup(&mut self, intent: ModelSetupIntent, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    let Some(dispatch) = self.state.begin_model_setup_command(intent) else {
      return;
    };
    let token = dispatch.token;
    // `dispatch_for_server` synchronously enqueues before returning its waiter.
    // Replacing this GPUI task may drop the waiter, but cannot retract or retarget the command.
    let response =
      client.dispatch_for_server(dispatch.expected_server_instance_id, dispatch.command);
    self.sync_composer(cx);
    cx.notify();
    self.model_setup_command_task = Some(cx.spawn(async move |cockpit, cx| {
      let result = response.await;
      let _ = cockpit.update(cx, |cockpit, cx| {
        let error = result.as_ref().err().map(ToString::to_string);
        cockpit
          .state
          .finish_model_setup_command(token, error.as_deref());
        cockpit.sync_composer(cx);
        cx.notify();
      });
    }));
  }

  pub(super) fn model_setup_control(
    &self,
    control: ModelSetupControl,
    cx: &mut Context<Self>,
  ) -> gpui::AnyElement {
    let enabled = control.enabled && self.client.is_some();
    let intent = control.intent;
    let accent = if intent == ModelSetupIntent::Cancel {
      APPROVAL_AMBER
    } else {
      SIGNAL_CYAN
    };
    div()
      .id(control.id)
      .px_3()
      .py_2()
      .rounded_sm()
      .border_1()
      .border_color(color(accent))
      .bg(color(DEEP_PANEL))
      .text_xs()
      .font_weight(gpui::FontWeight::SEMIBOLD)
      .text_color(color(accent))
      .when(enabled, |button| {
        button
          .cursor_pointer()
          .active(|style| style.opacity(0.78))
          .on_click(cx.listener(move |this, _, _, cx| {
            this.dispatch_model_setup(intent, cx);
          }))
      })
      .when(!enabled, |button| button.opacity(0.42))
      .child(control.label)
      .into_any_element()
  }
}

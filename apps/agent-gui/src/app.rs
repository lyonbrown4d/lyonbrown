use agent_client::NativeAgentClient;
use gpui::{Context, Entity, Render, Task, Window, div, prelude::*, px};

use crate::{
  client_state::{RemoteStatePump, apply_remote_state},
  composer_input::{ComposerInput, SubmitRequested},
  state::{CockpitState, RuntimeDetailsPlacement},
  theme::{GRAPHITE, SIGNAL_CYAN, color},
  view,
};

#[derive(Default)]
pub struct Cockpit {
  state: CockpitState,
  client: Option<NativeAgentClient>,
  remote_state_task: Option<Task<()>>,
  command_task: Option<Task<()>>,
  composer_input: Option<Entity<ComposerInput>>,
}

impl Cockpit {
  pub(crate) fn new(state: CockpitState, client: Option<NativeAgentClient>) -> Self {
    Self {
      state,
      client,
      remote_state_task: None,
      command_task: None,
      composer_input: None,
    }
  }

  pub fn state(&self) -> &CockpitState {
    &self.state
  }

  pub fn has_live_client(&self) -> bool {
    self.client.is_some()
  }

  pub fn start_remote_state_watch(&mut self, cx: &mut Context<Self>) {
    self.initialize_composer(cx);
    if self.remote_state_task.is_some() {
      return;
    }
    let Some(client) = self.client.as_ref() else {
      return;
    };

    let mut pump = RemoteStatePump::new(client.subscribe_remote_state());
    pump.apply_current(&mut self.state);
    self.sync_composer(cx);
    cx.notify();
    self.remote_state_task = Some(cx.spawn(async move |cockpit, cx| {
      while let Some(remote) = pump.next().await {
        if cockpit
          .update(cx, |cockpit, cx| {
            apply_remote_state(&mut cockpit.state, &remote);
            cockpit.sync_composer(cx);
            cx.notify();
          })
          .is_err()
        {
          break;
        }
      }
    }));
  }

  fn initialize_composer(&mut self, cx: &mut Context<Self>) {
    if self.composer_input.is_some() {
      return;
    }
    let input = cx.new(ComposerInput::new);
    cx.subscribe(&input, |cockpit, _, _: &SubmitRequested, cx| {
      cockpit.submit_prompt(cx);
    })
    .detach();
    self.composer_input = Some(input);
    self.sync_composer(cx);
  }

  fn sync_composer(&self, cx: &mut Context<Self>) {
    let Some(input) = self.composer_input.as_ref() else {
      return;
    };
    let enabled = self.state.can_submit() && self.client.is_some();
    input.update(cx, |input, cx| input.set_enabled(enabled, cx));
  }

  fn submit_prompt(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(input)) = (self.client.clone(), self.composer_input.clone()) else {
      return;
    };
    let prompt = input.read(cx).text().to_owned();
    let Some(command) = self.state.begin_submission(&prompt) else {
      return;
    };
    self.sync_composer(cx);
    cx.notify();
    self.command_task = Some(cx.spawn(async move |cockpit, cx| {
      let result = client.dispatch(command).await;
      let _ = cockpit.update(cx, |cockpit, cx| {
        if result.is_ok() {
          input.update(cx, |input, cx| {
            input.take_text(cx);
          });
        }
        let error = result.as_ref().err().map(ToString::to_string);
        cockpit.state.finish_submission(error.as_deref());
        cockpit.sync_composer(cx);
        cx.notify();
      });
    }));
  }
}

impl Render for Cockpit {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let compact = window.bounds().size.width < px(900.);
    let details_expanded = self.state.runtime_details_expanded();
    let details_placement = RuntimeDetailsPlacement::for_layout(compact, details_expanded);
    let label = self.state.runtime_details_control_label();
    let connection = self.state.connection();
    let stale = self.state.is_stale();
    let can_submit = self.state.can_submit();
    let model_setup_required = self.state.is_model_setup_required();
    let connection_error = self.state.last_connection_error();
    let command_error = self.state.last_command_error();
    let submitting = self.state.is_submitting();

    let composer_input = self
      .composer_input
      .as_ref()
      .map(|input| input.clone().into_any_element())
      .unwrap_or_else(|| div().child("Composer initializing…").into_any_element());

    let submit_control = div()
      .id("composer-submit")
      .px_3()
      .py_2()
      .rounded_sm()
      .bg(color(SIGNAL_CYAN))
      .text_color(color(GRAPHITE))
      .font_weight(gpui::FontWeight::SEMIBOLD)
      .when(can_submit, |button| {
        button
          .cursor_pointer()
          .active(|style| style.opacity(0.78))
          .on_click(cx.listener(|this, _, _, cx| this.submit_prompt(cx)))
      })
      .when(!can_submit, |button| button.opacity(0.55))
      .child(if submitting { "Sending…" } else { "Send ↵" })
      .into_any_element();

    let details_control = div()
      .id("runtime-details-control")
      .px_4()
      .py_2()
      .rounded_sm()
      .bg(color(SIGNAL_CYAN))
      .text_color(color(GRAPHITE))
      .font_weight(gpui::FontWeight::SEMIBOLD)
      .cursor_pointer()
      .active(|style| style.opacity(0.78))
      .child(label)
      .on_click(cx.listener(|this, _, _, cx| {
        this.state.toggle_runtime_details();
        cx.notify();
      }))
      .into_any_element();

    view::cockpit(view::CockpitView {
      compact,
      details_placement,
      connection,
      stale,
      can_submit,
      model_setup_required,
      connection_error,
      submitting,
      command_error,
      composer_input,
      submit_control,
      details_control,
    })
  }
}

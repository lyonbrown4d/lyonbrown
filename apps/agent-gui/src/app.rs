use agent_client::NativeAgentClient;
use gpui::{Context, Render, Task, Window, div, prelude::*, px};

use crate::{
  client_state::{RemoteStatePump, apply_remote_state},
  state::{CockpitState, RuntimeDetailsPlacement},
  theme::{GRAPHITE, SIGNAL_CYAN, color},
  view,
};

#[derive(Default)]
pub struct Cockpit {
  state: CockpitState,
  client: Option<NativeAgentClient>,
  remote_state_task: Option<Task<()>>,
}

impl Cockpit {
  pub(crate) fn new(state: CockpitState, client: Option<NativeAgentClient>) -> Self {
    Self {
      state,
      client,
      remote_state_task: None,
    }
  }

  pub fn state(&self) -> &CockpitState {
    &self.state
  }

  pub fn has_live_client(&self) -> bool {
    self.client.is_some()
  }

  pub fn start_remote_state_watch(&mut self, cx: &mut Context<Self>) {
    if self.remote_state_task.is_some() {
      return;
    }
    let Some(client) = self.client.as_ref() else {
      return;
    };

    let mut pump = RemoteStatePump::new(client.subscribe_remote_state());
    pump.apply_current(&mut self.state);
    cx.notify();
    self.remote_state_task = Some(cx.spawn(async move |cockpit, cx| {
      while let Some(remote) = pump.next().await {
        if cockpit
          .update(cx, |cockpit, cx| {
            apply_remote_state(&mut cockpit.state, &remote);
            cx.notify();
          })
          .is_err()
        {
          break;
        }
      }
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

    view::cockpit(
      compact,
      details_placement,
      connection,
      stale,
      can_submit,
      details_control,
    )
  }
}

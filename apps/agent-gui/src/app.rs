use gpui::{Context, Render, Window, div, prelude::*, px};

use crate::{
  state::{CockpitState, RuntimeDetailsPlacement},
  theme::{GRAPHITE, SIGNAL_CYAN, color},
  view,
};

#[derive(Default)]
pub struct Cockpit {
  state: CockpitState,
}

impl Render for Cockpit {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let compact = window.bounds().size.width < px(900.);
    let details_expanded = self.state.runtime_details_expanded();
    let details_placement = RuntimeDetailsPlacement::for_layout(compact, details_expanded);
    let label = self.state.runtime_details_control_label();

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

    view::cockpit(compact, details_placement, details_control)
  }
}

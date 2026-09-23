mod conversation;
mod runtime;
mod sessions;

use gpui::{AnyElement, div, prelude::*, px};

use crate::{
  state::RuntimeDetailsPlacement,
  theme::{DEEP_PANEL, GRAPHITE, HAIRLINE, MUTED_STEEL, PAPER, UI_FONT, color},
};

pub fn cockpit(
  compact: bool,
  details_placement: RuntimeDetailsPlacement,
  details_control: AnyElement,
) -> impl IntoElement {
  div()
    .size_full()
    .flex()
    .flex_col()
    .bg(color(GRAPHITE))
    .font_family(UI_FONT)
    .text_color(color(PAPER))
    .child(top_bar(compact))
    .child(
      div()
        .flex()
        .flex_1()
        .min_h_0()
        .when(!compact, |layout| layout.child(sessions::session_rail()))
        .child(conversation::conversation(
          details_placement == RuntimeDetailsPlacement::Conversation,
          details_control,
        ))
        .when(!compact, |layout| {
          layout.child(runtime::signal_rail(
            details_placement == RuntimeDetailsPlacement::RuntimeRail,
          ))
        }),
    )
}

fn top_bar(compact: bool) -> impl IntoElement {
  div()
    .h(px(46.))
    .flex_none()
    .flex()
    .items_center()
    .justify_between()
    .px_4()
    .border_b_1()
    .border_color(color(HAIRLINE))
    .bg(color(DEEP_PANEL))
    .child(
      div()
        .flex()
        .items_center()
        .gap_2()
        .child(
          div()
            .text_sm()
            .font_weight(gpui::FontWeight::BOLD)
            .child("LYONBROWN"),
        )
        .child(
          div()
            .text_xs()
            .text_color(color(MUTED_STEEL))
            .child("LOCAL AGENT COCKPIT"),
        ),
    )
    .child(
      div()
        .flex()
        .gap_2()
        .text_xs()
        .text_color(color(PAPER))
        .child(if compact { "COMPACT" } else { "3-RAIL" })
        .child("/")
        .child("NOT CONNECTED"),
    )
}

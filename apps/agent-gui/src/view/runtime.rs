use gpui::{div, prelude::*, px};

use crate::{
  state::ConnectionState,
  theme::{DATA_FONT, HAIRLINE, MUTED_STEEL, PANEL, PAPER, SIGNAL_CYAN, color},
};

pub fn signal_rail(
  details_expanded: bool,
  connection: ConnectionState,
  stale: bool,
  can_submit: bool,
  model_setup_required: bool,
) -> impl IntoElement {
  let signal_status = if model_setup_required {
    "model setup required"
  } else if can_submit {
    "snapshot available"
  } else {
    "unavailable"
  };
  let sync_status = if can_submit {
    "fresh snapshot"
  } else if stale {
    "stale snapshot"
  } else {
    "snapshot pending"
  };
  div()
    .w(px(238.))
    .flex_none()
    .flex()
    .flex_col()
    .p_4()
    .gap_4()
    .bg(color(PANEL))
    .border_l_1()
    .border_color(color(HAIRLINE))
    .child(
      div()
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child("RUNTIME SIGNAL"),
    )
    .child(signal("MODEL", signal_status))
    .child(signal("TOOL", signal_status))
    .child(signal("CONTEXT", signal_status))
    .child(signal("APPROVAL", signal_status))
    .when(details_expanded, |rail| {
      rail.child(preview_details(connection, can_submit))
    })
    .child(div().flex_1())
    .child(
      div()
        .font_family(DATA_FONT)
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child(format!(
          "{} / {sync_status}",
          super::connection_label(connection).to_ascii_lowercase()
        )),
    )
}

fn signal(name: &'static str, detail: &'static str) -> impl IntoElement {
  div()
    .flex()
    .gap_3()
    .child(
      div()
        .flex()
        .flex_col()
        .items_center()
        .child(div().size_2().rounded_full().bg(color(HAIRLINE)))
        .child(div().w(px(1.)).h(px(34.)).bg(color(HAIRLINE))),
    )
    .child(
      div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().text_color(color(PAPER)).child(name))
        .child(
          div()
            .font_family(DATA_FONT)
            .text_xs()
            .text_color(color(MUTED_STEEL))
            .child(detail),
        ),
    )
}

fn preview_details(connection: ConnectionState, can_submit: bool) -> impl IntoElement {
  let command_status = if can_submit {
    "commands enabled"
  } else {
    "commands disabled"
  };
  div()
    .p_3()
    .rounded_sm()
    .border_1()
    .border_color(color(SIGNAL_CYAN))
    .font_family(DATA_FONT)
    .text_xs()
    .text_color(color(MUTED_STEEL))
    .child(format!(
      "{} · {command_status}",
      super::connection_label(connection)
    ))
}

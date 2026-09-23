use gpui::{div, prelude::*, px};

use crate::theme::{DATA_FONT, HAIRLINE, MUTED_STEEL, PANEL, PAPER, SIGNAL_CYAN, color};

pub fn signal_rail(details_expanded: bool) -> impl IntoElement {
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
    .child(signal("MODEL", "unavailable"))
    .child(signal("TOOL", "unavailable"))
    .child(signal("CONTEXT", "unavailable"))
    .child(signal("APPROVAL", "unavailable"))
    .when(details_expanded, |rail| rail.child(preview_details()))
    .child(div().flex_1())
    .child(
      div()
        .font_family(DATA_FONT)
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child("no pulse / runtime not connected"),
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

fn preview_details() -> impl IntoElement {
  div()
    .p_3()
    .rounded_sm()
    .border_1()
    .border_color(color(SIGNAL_CYAN))
    .font_family(DATA_FONT)
    .text_xs()
    .text_color(color(MUTED_STEEL))
    .child("PREVIEW ONLY · no runtime provider attached")
}

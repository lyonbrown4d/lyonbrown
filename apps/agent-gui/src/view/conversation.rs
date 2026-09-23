use gpui::{AnyElement, div, prelude::*};

use crate::theme::{DATA_FONT, DEEP_PANEL, HAIRLINE, MUTED_STEEL, PANEL, color};

pub fn conversation(show_runtime_details: bool, details_control: AnyElement) -> impl IntoElement {
  div()
    .flex_1()
    .min_w_0()
    .flex()
    .flex_col()
    .child(
      div()
        .flex_1()
        .min_h_0()
        .p_5()
        .flex()
        .flex_col()
        .gap_4()
        .child(task_message())
        .child(agent_message()),
    )
    .when(show_runtime_details, |column| {
      column.child(compact_runtime_preview())
    })
    .child(composer(details_control))
}

fn compact_runtime_preview() -> impl IntoElement {
  div()
    .mx_4()
    .p_3()
    .rounded_sm()
    .border_1()
    .border_color(color(crate::theme::SIGNAL_CYAN))
    .bg(color(DEEP_PANEL))
    .font_family(DATA_FONT)
    .text_xs()
    .text_color(color(MUTED_STEEL))
    .child("RUNTIME PREVIEW · NOT CONNECTED · all signals unavailable")
}

fn task_message() -> impl IntoElement {
  div()
    .flex()
    .flex_col()
    .gap_2()
    .child(
      div()
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child("SAMPLE TASK"),
    )
    .child(
      div()
        .max_w(gpui::px(720.))
        .p_4()
        .rounded_sm()
        .bg(color(PANEL))
        .border_1()
        .border_color(color(HAIRLINE))
        .text_sm()
        .child("Create a native desktop cockpit for a local-first coding agent."),
    )
}

fn agent_message() -> impl IntoElement {
  div()
    .flex()
    .flex_col()
    .gap_2()
    .child(
      div()
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child("AGENT UI PREVIEW"),
    )
    .child(
      div()
        .max_w(gpui::px(720.))
        .p_4()
        .rounded_sm()
        .bg(color(DEEP_PANEL))
        .border_l_2()
        .border_color(color(crate::theme::SIGNAL_CYAN))
        .child("The shell is ready. Runtime integration is intentionally not connected yet."),
    )
    .child(
      div()
        .font_family(DATA_FONT)
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child("workspace  unavailable until runtime is connected"),
    )
}

fn composer(details_control: AnyElement) -> impl IntoElement {
  div()
    .p_4()
    .border_t_1()
    .border_color(color(HAIRLINE))
    .bg(color(DEEP_PANEL))
    .child(
      div()
        .flex()
        .items_center()
        .gap_3()
        .p_3()
        .rounded_sm()
        .bg(color(PANEL))
        .border_1()
        .border_color(color(HAIRLINE))
        .child(
          div()
            .flex_1()
            .text_sm()
            .text_color(color(MUTED_STEEL))
            .child("Describe the next task…"),
        )
        .child(details_control),
    )
    .child(
      div()
        .mt_2()
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child("Runtime not connected · control previews UI details only"),
    )
}

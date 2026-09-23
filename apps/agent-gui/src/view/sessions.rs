use gpui::{div, prelude::*, px};

use crate::theme::{HAIRLINE, MUTED_STEEL, PANEL, PAPER, SIGNAL_CYAN, color};

pub fn session_rail() -> impl IntoElement {
  div()
    .w(px(196.))
    .flex_none()
    .flex()
    .flex_col()
    .p_3()
    .gap_2()
    .bg(color(PANEL))
    .border_r_1()
    .border_color(color(HAIRLINE))
    .child(
      div()
        .mb_2()
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child("SESSIONS / PREVIEW"),
    )
    .child(session("01", "GUI shell", "now", true))
    .child(session("02", "Index workspace", "18m", false))
    .child(session("03", "Review patches", "1h", false))
    .child(div().flex_1())
    .child(
      div()
        .pt_3()
        .border_t_1()
        .border_color(color(HAIRLINE))
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child("3 sample sessions"),
    )
}

fn session(
  index: &'static str,
  title: &'static str,
  age: &'static str,
  active: bool,
) -> impl IntoElement {
  let accent = if active { SIGNAL_CYAN } else { HAIRLINE };
  div()
    .flex()
    .items_center()
    .gap_2()
    .p_2()
    .rounded_sm()
    .border_l_2()
    .border_color(color(accent))
    .when(active, |item| item.bg(color(0x1c2e38)))
    .child(div().text_xs().text_color(color(MUTED_STEEL)).child(index))
    .child(
      div()
        .flex_1()
        .text_sm()
        .text_color(color(PAPER))
        .child(title),
    )
    .child(div().text_xs().text_color(color(MUTED_STEEL)).child(age))
}

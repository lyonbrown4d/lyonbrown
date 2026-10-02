use gpui::{AnyElement, div, prelude::*};

use crate::{
  copy::{Copy, UiLocale},
  model_setup::{ModelSetupDetail, ModelSetupDetailKind, ModelSetupPresentation},
  theme::{APPROVAL_AMBER, DATA_FONT, MUTED_STEEL, PANEL, PAPER, SIGNAL_CYAN, color},
};

pub(super) struct ModelSetupCardView {
  pub presentation: ModelSetupPresentation,
  pub primary_control: Option<AnyElement>,
  pub cancel_control: Option<AnyElement>,
  pub locale: UiLocale,
}

pub(super) fn card(view: ModelSetupCardView) -> impl IntoElement {
  let copy = Copy::new(view.locale);
  let presentation = view.presentation;
  div()
    .mx_4()
    .mb_1()
    .p_4()
    .flex()
    .flex_col()
    .gap_3()
    .rounded_sm()
    .border_1()
    .border_color(color(APPROVAL_AMBER))
    .bg(color(PANEL))
    .child(
      div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
          div()
            .text_xs()
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(color(APPROVAL_AMBER))
            .child(presentation.title),
        )
        .child(
          div()
            .font_family(DATA_FONT)
            .text_xs()
            .text_color(color(MUTED_STEEL))
            .child(presentation.source),
        ),
    )
    .child(
      div()
        .font_family(DATA_FONT)
        .text_sm()
        .text_color(color(PAPER))
        .child(presentation.phase),
    )
    .children(
      presentation
        .details
        .into_iter()
        .map(|detail| detail_line(detail, copy)),
    )
    .when(
      view.primary_control.is_some() || view.cancel_control.is_some(),
      |card| {
        card.child(
          div()
            .flex()
            .items_center()
            .gap_2()
            .children(view.primary_control)
            .children(view.cancel_control),
        )
      },
    )
}

fn detail_line(detail: ModelSetupDetail, copy: Copy) -> impl IntoElement {
  let (label, accent) = match detail.kind {
    ModelSetupDetailKind::Progress => (copy.text("PROGRESS", "进度"), SIGNAL_CYAN),
    ModelSetupDetailKind::Model => (copy.text("MODEL", "模型"), MUTED_STEEL),
    ModelSetupDetailKind::Operation => (copy.text("OPERATION", "操作"), MUTED_STEEL),
    ModelSetupDetailKind::Failure => (copy.text("FAILURE", "失败"), APPROVAL_AMBER),
    ModelSetupDetailKind::CommandError => (copy.text("ERROR", "错误"), APPROVAL_AMBER),
  };
  status_line(label, detail.value, accent)
}

fn status_line(label: &'static str, value: String, accent: u32) -> impl IntoElement {
  div()
    .flex()
    .gap_3()
    .border_l_2()
    .border_color(color(accent))
    .pl_3()
    .font_family(DATA_FONT)
    .text_xs()
    .child(
      div()
        .w(gpui::px(76.))
        .flex_none()
        .text_color(color(MUTED_STEEL))
        .child(label),
    )
    .child(div().min_w_0().text_color(color(PAPER)).child(value))
}

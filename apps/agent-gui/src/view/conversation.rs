use gpui::{AnyElement, div, prelude::*};

use crate::{
  state::ConnectionState,
  theme::{APPROVAL_AMBER, DATA_FONT, DEEP_PANEL, HAIRLINE, MUTED_STEEL, PANEL, color},
};

pub(super) struct ConversationView<'a> {
  pub show_runtime_details: bool,
  pub connection: ConnectionState,
  pub stale: bool,
  pub can_submit: bool,
  pub model_setup_required: bool,
  pub submitting: bool,
  pub command_error: Option<&'a str>,
  pub composer_input: AnyElement,
  pub submit_control: AnyElement,
  pub details_control: AnyElement,
}

pub fn conversation(view: ConversationView<'_>) -> impl IntoElement {
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
        .child(agent_message(view.connection)),
    )
    .when(view.show_runtime_details, |column| {
      column.child(compact_runtime_preview(
        view.connection,
        view.stale,
        view.can_submit,
      ))
    })
    .child(composer(ComposerView {
      connection: view.connection,
      can_submit: view.can_submit,
      model_setup_required: view.model_setup_required,
      submitting: view.submitting,
      command_error: view.command_error,
      composer_input: view.composer_input,
      submit_control: view.submit_control,
      details_control: view.details_control,
    }))
}

fn compact_runtime_preview(
  connection: ConnectionState,
  stale: bool,
  can_submit: bool,
) -> impl IntoElement {
  let sync_status = if can_submit {
    "fresh snapshot"
  } else if stale {
    "stale snapshot"
  } else {
    "snapshot pending"
  };
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
    .child(format!(
      "RUNTIME PREVIEW · {} · {sync_status}",
      super::connection_label(connection)
    ))
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

fn agent_message(connection: ConnectionState) -> impl IntoElement {
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
        .child("The shell is ready. Runtime commands remain gated by synchronized state."),
    )
    .child(
      div()
        .font_family(DATA_FONT)
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child(format!(
          "transport  {}",
          super::connection_label(connection).to_ascii_lowercase()
        )),
    )
}

struct ComposerView<'a> {
  connection: ConnectionState,
  can_submit: bool,
  model_setup_required: bool,
  submitting: bool,
  command_error: Option<&'a str>,
  composer_input: AnyElement,
  submit_control: AnyElement,
  details_control: AnyElement,
}

fn composer(view: ComposerView<'_>) -> impl IntoElement {
  let status = if let Some(error) = view.command_error {
    format!("COMMAND FAILED · {error}")
  } else if view.submitting {
    "SENDING · 正在提交远程任务…".to_owned()
  } else if view.can_submit {
    "Fresh runtime snapshot · composer ready".to_owned()
  } else if view.model_setup_required {
    "MODEL SETUP REQUIRED · choose a local model or configure another provider".to_owned()
  } else {
    format!(
      "{} · composer disabled until a fresh snapshot is available",
      super::connection_label(view.connection)
    )
  };
  let status_color = if view.command_error.is_some() {
    APPROVAL_AMBER
  } else {
    MUTED_STEEL
  };
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
        .when(!view.can_submit, |composer| composer.opacity(0.55))
        .child(
          div()
            .flex_1()
            .text_sm()
            .text_color(color(MUTED_STEEL))
            .child(view.composer_input),
        )
        .child(view.submit_control)
        .child(view.details_control),
    )
    .child(
      div()
        .mt_2()
        .text_xs()
        .text_color(color(status_color))
        .child(status),
    )
}

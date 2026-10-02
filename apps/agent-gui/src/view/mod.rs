mod conversation;
mod runtime;
mod sessions;

use gpui::{AnyElement, div, prelude::*, px};

use crate::{
  state::{ConnectionState, RuntimeDetailsPlacement},
  theme::{
    APPROVAL_AMBER, DATA_FONT, DEEP_PANEL, GRAPHITE, HAIRLINE, MUTED_STEEL, PAPER, UI_FONT, color,
  },
};

pub struct CockpitView<'a> {
  pub compact: bool,
  pub details_placement: RuntimeDetailsPlacement,
  pub connection: ConnectionState,
  pub stale: bool,
  pub can_submit: bool,
  pub model_setup_required: bool,
  pub connection_error: Option<&'a str>,
  pub submitting: bool,
  pub command_error: Option<&'a str>,
  pub composer_input: AnyElement,
  pub submit_control: AnyElement,
  pub details_control: AnyElement,
}

pub fn cockpit(view: CockpitView<'_>) -> impl IntoElement {
  let banner = connection_banner_text(view.connection, view.stale, view.connection_error);
  let mut shell = div()
    .size_full()
    .flex()
    .flex_col()
    .bg(color(GRAPHITE))
    .font_family(UI_FONT)
    .text_color(color(PAPER))
    .child(top_bar(view.compact, view.connection));
  if let Some(text) = banner {
    shell = shell.child(connection_banner(text));
  }
  shell.child(
    div()
      .flex()
      .flex_1()
      .min_h_0()
      .when(!view.compact, |layout| {
        layout.child(sessions::session_rail())
      })
      .child(conversation::conversation(conversation::ConversationView {
        show_runtime_details: view.details_placement == RuntimeDetailsPlacement::Conversation,
        connection: view.connection,
        stale: view.stale,
        can_submit: view.can_submit,
        model_setup_required: view.model_setup_required,
        submitting: view.submitting,
        command_error: view.command_error,
        composer_input: view.composer_input,
        submit_control: view.submit_control,
        details_control: view.details_control,
      }))
      .when(!view.compact, |layout| {
        layout.child(runtime::signal_rail(
          view.details_placement == RuntimeDetailsPlacement::RuntimeRail,
          view.connection,
          view.stale,
          view.can_submit,
          view.model_setup_required,
        ))
      }),
  )
}

pub fn connection_banner_text(
  connection: ConnectionState,
  stale: bool,
  error: Option<&str>,
) -> Option<String> {
  if connection == ConnectionState::Connected && !stale && error.is_none() {
    return None;
  }
  let sync = if stale {
    "cached content is stale"
  } else {
    "fresh snapshot pending"
  };
  let action = match connection {
    ConnectionState::Reconnecting => "Reconnecting automatically",
    ConnectionState::Unavailable => "Retry or restart agentd",
    ConnectionState::Incompatible => "Update the client or restart agentd",
    ConnectionState::Disconnected | ConnectionState::Connecting | ConnectionState::Connected => {
      "Waiting for agentd"
    }
  };
  let detail = error.unwrap_or("runtime connection is not ready");
  Some(format!(
    "{} · {sync} · {detail} · {action}",
    connection_label(connection)
  ))
}

fn connection_banner(text: String) -> impl IntoElement {
  div()
    .flex_none()
    .px_4()
    .py_2()
    .border_b_1()
    .border_color(color(APPROVAL_AMBER))
    .bg(color(DEEP_PANEL))
    .font_family(DATA_FONT)
    .text_xs()
    .text_color(color(APPROVAL_AMBER))
    .child(text)
}

pub(super) fn connection_label(connection: ConnectionState) -> &'static str {
  match connection {
    ConnectionState::Disconnected => "DISCONNECTED",
    ConnectionState::Connecting => "CONNECTING",
    ConnectionState::Connected => "CONNECTED",
    ConnectionState::Reconnecting => "RECONNECTING",
    ConnectionState::Unavailable => "UNAVAILABLE",
    ConnectionState::Incompatible => "INCOMPATIBLE",
  }
}

fn top_bar(compact: bool, connection: ConnectionState) -> impl IntoElement {
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
        .child(connection_label(connection)),
    )
}

use gpui::{AnyElement, div, prelude::*};

use crate::{
  copy::{Copy, UiLocale},
  model_setup::ModelSetupPresentation,
  state::ConnectionState,
  theme::{APPROVAL_AMBER, DATA_FONT, DEEP_PANEL, HAIRLINE, MUTED_STEEL, PANEL, color},
};

pub(super) struct ConversationView<'a> {
  pub show_runtime_details: bool,
  pub connection: ConnectionState,
  pub stale: bool,
  pub snapshot_fresh: bool,
  pub can_submit: bool,
  pub model_setup: Option<ModelSetupPresentation>,
  pub model_setup_primary: Option<AnyElement>,
  pub model_setup_cancel: Option<AnyElement>,
  pub locale: UiLocale,
  pub submitting: bool,
  pub command_error: Option<&'a str>,
  pub composer_input: AnyElement,
  pub submit_control: AnyElement,
  pub details_control: AnyElement,
}

pub fn conversation(view: ConversationView<'_>) -> impl IntoElement {
  let model_setup_required = view.model_setup.is_some();
  let shell = super::shell_copy(view.locale);
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
        .child(task_message(shell))
        .child(agent_message(view.connection, view.locale, shell)),
    )
    .when(view.show_runtime_details, |column| {
      column.child(compact_runtime_preview(
        view.connection,
        view.stale,
        view.snapshot_fresh,
        view.locale,
      ))
    })
    .when_some(view.model_setup, |column, presentation| {
      column.child(super::model_setup::card(
        super::model_setup::ModelSetupCardView {
          presentation,
          primary_control: view.model_setup_primary,
          cancel_control: view.model_setup_cancel,
          locale: view.locale,
        },
      ))
    })
    .child(composer(ComposerView {
      connection: view.connection,
      can_submit: view.can_submit,
      model_setup_required,
      locale: view.locale,
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
  snapshot_fresh: bool,
  locale: UiLocale,
) -> impl IntoElement {
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
    .child(super::runtime_preview_text(
      connection,
      stale,
      snapshot_fresh,
      locale,
    ))
}

fn task_message(copy: super::ShellCopy) -> impl IntoElement {
  div()
    .flex()
    .flex_col()
    .gap_2()
    .child(
      div()
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child(copy.sample_task),
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
        .child(copy.sample_task_body),
    )
}

fn agent_message(
  connection: ConnectionState,
  locale: UiLocale,
  copy: super::ShellCopy,
) -> impl IntoElement {
  div()
    .flex()
    .flex_col()
    .gap_2()
    .child(
      div()
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child(copy.agent_preview),
    )
    .child(
      div()
        .max_w(gpui::px(720.))
        .p_4()
        .rounded_sm()
        .bg(color(DEEP_PANEL))
        .border_l_2()
        .border_color(color(crate::theme::SIGNAL_CYAN))
        .child(copy.agent_preview_body),
    )
    .child(
      div()
        .font_family(DATA_FONT)
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child(format!(
          "{}  {}",
          copy.transport,
          super::connection_label(connection, locale).to_ascii_lowercase()
        )),
    )
}

struct ComposerView<'a> {
  connection: ConnectionState,
  can_submit: bool,
  model_setup_required: bool,
  locale: UiLocale,
  submitting: bool,
  command_error: Option<&'a str>,
  composer_input: AnyElement,
  submit_control: AnyElement,
  details_control: AnyElement,
}

fn composer(view: ComposerView<'_>) -> impl IntoElement {
  let copy = Copy::new(view.locale);
  let status = if let Some(error) = view.command_error {
    format!(
      "{} · {}",
      copy.text("COMMAND FAILED", "命令失败"),
      crate::copy::command_error_text(error, view.locale)
    )
  } else if view.submitting {
    copy
      .text(
        "SENDING · submitting remote task…",
        "正在发送 · 正在提交远程任务…",
      )
      .to_owned()
  } else if view.can_submit {
    copy
      .text(
        "Fresh runtime snapshot · composer ready",
        "运行时快照已同步 · 输入已就绪",
      )
      .to_owned()
  } else if view.model_setup_required {
    copy
      .text(
        "MODEL SETUP REQUIRED · finish setup above to enable chat",
        "需要配置模型 · 完成上方配置后即可聊天",
      )
      .to_owned()
  } else {
    format!(
      "{} · {}",
      super::connection_label(view.connection, view.locale),
      copy.text(
        "composer disabled until a fresh snapshot is available",
        "等待最新快照，输入暂不可用"
      )
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

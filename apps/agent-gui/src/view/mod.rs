mod conversation;
mod model_setup;
mod runtime;
mod sessions;

use agent_app::RuntimePhase;
use gpui::{AnyElement, div, prelude::*, px};

use crate::{
  copy::{Copy, UiLocale},
  model_setup::ModelSetupPresentation,
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
  pub snapshot_fresh: bool,
  pub can_submit: bool,
  pub runtime_phase: RuntimePhase,
  pub model_setup: Option<ModelSetupPresentation>,
  pub model_setup_primary: Option<AnyElement>,
  pub model_setup_cancel: Option<AnyElement>,
  pub locale: UiLocale,
  pub connection_error: Option<&'a str>,
  pub submitting: bool,
  pub command_error: Option<&'a str>,
  pub composer_input: AnyElement,
  pub submit_control: AnyElement,
  pub details_control: AnyElement,
  pub locale_control: AnyElement,
}

pub fn cockpit(view: CockpitView<'_>) -> impl IntoElement {
  let banner = connection_banner_text(
    view.connection,
    view.stale,
    view.connection_error,
    view.locale,
  );
  let rail_setup = view.model_setup.clone();
  let mut shell = div()
    .size_full()
    .flex()
    .flex_col()
    .bg(color(GRAPHITE))
    .font_family(UI_FONT)
    .text_color(color(PAPER))
    .child(top_bar(
      view.compact,
      view.connection,
      view.locale,
      view.locale_control,
    ));
  if let Some(text) = banner {
    shell = shell.child(connection_banner(text));
  }
  shell.child(
    div()
      .flex()
      .flex_1()
      .min_h_0()
      .when(!view.compact, |layout| {
        layout.child(sessions::session_rail(view.locale))
      })
      .child(conversation::conversation(conversation::ConversationView {
        show_runtime_details: view.details_placement == RuntimeDetailsPlacement::Conversation,
        connection: view.connection,
        stale: view.stale,
        snapshot_fresh: view.snapshot_fresh,
        can_submit: view.can_submit,
        model_setup: view.model_setup,
        model_setup_primary: view.model_setup_primary,
        model_setup_cancel: view.model_setup_cancel,
        locale: view.locale,
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
          view.snapshot_fresh,
          view.runtime_phase,
          rail_setup.as_ref(),
          view.locale,
        ))
      }),
  )
}

pub fn connection_banner_text(
  connection: ConnectionState,
  stale: bool,
  error: Option<&str>,
  locale: UiLocale,
) -> Option<String> {
  if connection == ConnectionState::Connected && !stale && error.is_none() {
    return None;
  }
  let copy = Copy::new(locale);
  let sync = if stale {
    copy.text("cached content is stale", "缓存内容已过期")
  } else {
    copy.text("fresh snapshot pending", "等待最新快照")
  };
  let action = match connection {
    ConnectionState::Reconnecting => copy.text("Reconnecting automatically", "正在自动重连"),
    ConnectionState::Unavailable => copy.text("Retry or restart agentd", "请重试或重启 agentd"),
    ConnectionState::Incompatible => copy.text(
      "Update the client or restart agentd",
      "请更新客户端或重启 agentd",
    ),
    ConnectionState::Disconnected | ConnectionState::Connecting | ConnectionState::Connected => {
      copy.text("Waiting for agentd", "正在等待 agentd")
    }
  };
  let detail = error.unwrap_or(copy.text("runtime connection is not ready", "运行时连接尚未就绪"));
  Some(format!(
    "{} · {sync} · {detail} · {action}",
    connection_label(connection, locale)
  ))
}

pub fn composer_initializing_text(locale: UiLocale) -> &'static str {
  Copy::new(locale).text("Composer initializing…", "输入框初始化中…")
}

pub fn runtime_preview_text(
  connection: ConnectionState,
  stale: bool,
  snapshot_fresh: bool,
  locale: UiLocale,
) -> String {
  let copy = Copy::new(locale);
  let sync_status = if snapshot_fresh {
    copy.text("fresh snapshot", "最新快照")
  } else if stale {
    copy.text("stale snapshot", "过期快照")
  } else {
    copy.text("snapshot pending", "等待快照")
  };
  format!(
    "{} · {} · {sync_status}",
    copy.text("RUNTIME PREVIEW", "运行时预览"),
    connection_label(connection, locale)
  )
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

pub fn connection_label(connection: ConnectionState, locale: UiLocale) -> &'static str {
  let copy = Copy::new(locale);
  match connection {
    ConnectionState::Disconnected => copy.text("DISCONNECTED", "已断开"),
    ConnectionState::Connecting => copy.text("CONNECTING", "连接中"),
    ConnectionState::Connected => copy.text("CONNECTED", "已连接"),
    ConnectionState::Reconnecting => copy.text("RECONNECTING", "重新连接中"),
    ConnectionState::Unavailable => copy.text("UNAVAILABLE", "不可用"),
    ConnectionState::Incompatible => copy.text("INCOMPATIBLE", "不兼容"),
  }
}

fn top_bar(
  compact: bool,
  connection: ConnectionState,
  locale: UiLocale,
  locale_control: AnyElement,
) -> impl IntoElement {
  let shell = shell_copy(locale);
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
            .child(shell.subtitle),
        ),
    )
    .child(
      div()
        .flex()
        .gap_2()
        .text_xs()
        .text_color(color(PAPER))
        .child(if compact {
          shell.compact_layout
        } else {
          shell.three_rail_layout
        })
        .child("/")
        .child(connection_label(connection, locale))
        .child(locale_control),
    )
}

#[derive(Clone, Copy)]
pub struct ShellCopy {
  pub subtitle: &'static str,
  pub compact_layout: &'static str,
  pub three_rail_layout: &'static str,
  pub sample_task: &'static str,
  pub sample_task_body: &'static str,
  pub agent_preview: &'static str,
  pub agent_preview_body: &'static str,
  pub transport: &'static str,
  pub sessions_preview: &'static str,
  pub gui_shell: &'static str,
  pub index_workspace: &'static str,
  pub review_patches: &'static str,
  pub now: &'static str,
  pub sample_sessions: &'static str,
}

pub fn shell_copy(locale: UiLocale) -> ShellCopy {
  let copy = Copy::new(locale);
  ShellCopy {
    subtitle: copy.text("LOCAL AGENT COCKPIT", "本地 AGENT 控制台"),
    compact_layout: copy.text("COMPACT", "紧凑"),
    three_rail_layout: copy.text("3-RAIL", "三栏"),
    sample_task: copy.text("SAMPLE TASK", "示例任务"),
    sample_task_body: copy.text(
      "Create a native desktop cockpit for a local-first coding agent.",
      "为本地优先的编程 Agent 创建原生桌面控制台。",
    ),
    agent_preview: copy.text("AGENT UI PREVIEW", "AGENT 界面预览"),
    agent_preview_body: copy.text(
      "The shell is ready. Runtime commands remain gated by synchronized state.",
      "界面已就绪。运行时命令仍由同步状态严格门控。",
    ),
    transport: copy.text("transport", "传输"),
    sessions_preview: copy.text("SESSIONS / PREVIEW", "会话 / 预览"),
    gui_shell: copy.text("GUI shell", "GUI 界面"),
    index_workspace: copy.text("Index workspace", "索引工作区"),
    review_patches: copy.text("Review patches", "审查补丁"),
    now: copy.text("now", "刚刚"),
    sample_sessions: copy.text("3 sample sessions", "3 个示例会话"),
  }
}

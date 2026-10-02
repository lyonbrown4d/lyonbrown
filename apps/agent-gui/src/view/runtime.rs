use agent_app::RuntimePhase;
use gpui::{div, prelude::*, px};

use crate::{
  copy::{Copy, UiLocale},
  model_setup::ModelSetupPresentation,
  state::ConnectionState,
  theme::{DATA_FONT, HAIRLINE, MUTED_STEEL, PANEL, PAPER, SIGNAL_CYAN, color},
};

pub fn signal_rail(
  details_expanded: bool,
  connection: ConnectionState,
  stale: bool,
  snapshot_fresh: bool,
  phase: RuntimePhase,
  model_setup: Option<&ModelSetupPresentation>,
  locale: UiLocale,
) -> impl IntoElement {
  let copy = Copy::new(locale);
  let runtime_status = phase_label(phase, copy).to_owned();
  let model_status = if let Some(setup) = model_setup {
    match setup.progress.as_ref() {
      Some(progress) => format!("{} · {progress}", setup.phase),
      None => setup.phase.clone(),
    }
  } else if snapshot_fresh {
    copy.text("ready", "已就绪").to_owned()
  } else {
    copy.text("unavailable", "不可用").to_owned()
  };
  let sync_status = if snapshot_fresh {
    copy.text("fresh snapshot", "最新快照")
  } else if stale {
    copy.text("stale snapshot", "过期快照")
  } else {
    copy.text("snapshot pending", "等待快照")
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
        .child(copy.text("RUNTIME SIGNAL", "运行时状态")),
    )
    .child(signal(copy.text("MODEL", "模型"), model_status))
    .child(signal(
      copy.text("RUNTIME", "运行时"),
      runtime_status.clone(),
    ))
    .child(signal(
      copy.text("CONTEXT", "上下文"),
      runtime_status.clone(),
    ))
    .child(signal(copy.text("APPROVAL", "审批"), runtime_status))
    .when(details_expanded, |rail| {
      rail.child(preview_details(connection, snapshot_fresh, locale, copy))
    })
    .child(div().flex_1())
    .child(
      div()
        .font_family(DATA_FONT)
        .text_xs()
        .text_color(color(MUTED_STEEL))
        .child(format!(
          "{} / {sync_status}",
          super::connection_label(connection, locale).to_ascii_lowercase()
        )),
    )
}

fn signal(name: &'static str, detail: String) -> impl IntoElement {
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

fn preview_details(
  connection: ConnectionState,
  snapshot_fresh: bool,
  locale: UiLocale,
  copy: Copy,
) -> impl IntoElement {
  let command_status = if snapshot_fresh {
    copy.text("snapshot fresh", "快照最新")
  } else {
    copy.text("snapshot unavailable", "快照不可用")
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
      super::connection_label(connection, locale)
    ))
}

fn phase_label(phase: RuntimePhase, copy: Copy) -> &'static str {
  match phase {
    RuntimePhase::Initializing => copy.text("initializing", "初始化中"),
    RuntimePhase::ModelSetupRequired => copy.text("model setup required", "需要配置模型"),
    RuntimePhase::Ready => copy.text("ready", "已就绪"),
    RuntimePhase::Preparing => copy.text("preparing", "准备中"),
    RuntimePhase::Streaming => copy.text("streaming", "生成中"),
    RuntimePhase::AwaitingApproval => copy.text("awaiting approval", "等待审批"),
    RuntimePhase::RunningTool => copy.text("running tool", "工具执行中"),
    RuntimePhase::Cancelling => copy.text("cancelling", "取消中"),
    RuntimePhase::Completed => copy.text("completed", "已完成"),
    RuntimePhase::FailedRecoverable => copy.text("recoverable failure", "可恢复失败"),
    RuntimePhase::FailedTerminal => copy.text("terminal failure", "终止失败"),
  }
}

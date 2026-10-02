use agent_app::{ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView};
use ratatui::{
  Frame,
  layout::Rect,
  style::Style,
  text::{Line, Text},
  widgets::{Paragraph, Wrap},
};

use super::{Locale, locale::Messages, status_line_safe, theme::Theme, ui::panel};

pub fn format_model_setup_progress(
  setup: &ModelSetupView,
  locale: Locale,
  inner_width: usize,
) -> String {
  format_progress_line(setup, locale.messages(), inner_width)
}

pub(super) struct ModelSetupRenderContext<'a> {
  pub command_error: Option<&'a str>,
  pub commands_enabled: bool,
  pub command_pending: bool,
  pub exit_pending: bool,
  pub theme: Theme,
  pub messages: Messages,
}

pub(super) fn render_model_setup(
  frame: &mut Frame<'_>,
  area: Rect,
  setup: &ModelSetupView,
  context: ModelSetupRenderContext<'_>,
) {
  let setup_messages = context.messages.setup;
  let mut lines = Vec::new();
  lines.push(Line::from(format!(
    "{}: {}",
    setup_messages.phase,
    phase_label(setup.phase, context.messages)
  )));
  lines.push(Line::from(action_hint(setup, &context)));

  if let Some(failure) = setup.failure.as_ref() {
    lines.push(Line::from(format!(
      "{}: {}",
      setup_messages.failure_code,
      status_line_safe(&failure.code)
    )));
    lines.push(Line::from(format!(
      "{}: {}",
      setup_messages.failure_message,
      status_line_safe(&failure.message)
    )));
  }
  if let Some(error) = context.command_error {
    lines.push(Line::from(format!(
      "{}: {}",
      setup_messages.command_error,
      status_line_safe(error)
    )));
  }
  if shows_progress(setup.phase) {
    lines.push(Line::from(format_progress_line(
      setup,
      context.messages,
      usize::from(area.width.saturating_sub(2)),
    )));
  }
  if setup.source == ModelSetupSource::ConfiguredPath {
    lines.push(Line::from(setup_messages.configured_path));
  }
  if let Some(model) = setup.managed_model.as_ref() {
    lines.push(Line::from(format!(
      "{}: {}",
      setup_messages.model_id,
      status_line_safe(&model.id)
    )));
    lines.push(Line::from(format!(
      "{}: {}",
      setup_messages.estimated_download,
      format_bytes(model.expected_download_bytes)
    )));
  }
  if let Some(operation_id) = setup.operation_id.as_ref() {
    lines.push(Line::from(format!(
      "{}: {}",
      setup_messages.operation_id,
      status_line_safe(operation_id)
    )));
  }

  let color = match setup.phase {
    ModelSetupPhase::Ready => context.theme.status_success,
    ModelSetupPhase::Failed => context.theme.status_danger,
    _ => context.theme.status_warning,
  };
  frame.render_widget(
    Paragraph::new(Text::from(lines))
      .block(panel(setup_messages.title, context.theme, false))
      .style(Style::default().fg(color))
      .wrap(Wrap { trim: false }),
    area,
  );
}

fn phase_label(phase: ModelSetupPhase, messages: Messages) -> &'static str {
  let setup = messages.setup;
  match phase {
    ModelSetupPhase::Required => setup.required,
    ModelSetupPhase::WaitingForLock => setup.waiting_for_lock,
    ModelSetupPhase::Checking => setup.checking,
    ModelSetupPhase::Downloading => setup.downloading,
    ModelSetupPhase::Verifying => setup.verifying,
    ModelSetupPhase::Installing => setup.installing_locked,
    ModelSetupPhase::Activating => setup.activating_locked,
    ModelSetupPhase::Ready => setup.ready,
    ModelSetupPhase::Cancelled => setup.cancelled,
    ModelSetupPhase::Failed => setup.failed,
  }
}

fn action_hint(setup: &ModelSetupView, context: &ModelSetupRenderContext<'_>) -> &'static str {
  let text = context.messages.setup;
  if context.exit_pending {
    return text.exit_after_command;
  }
  if context.command_pending {
    return if matches!(
      setup.phase,
      ModelSetupPhase::Required | ModelSetupPhase::Failed | ModelSetupPhase::Cancelled
    ) {
      text.command_pending_escape
    } else {
      text.command_pending
    };
  }
  if !context.commands_enabled {
    return text.commands_disabled;
  }
  if setup.source == ModelSetupSource::ConfiguredPath {
    return text.exit_escape_hint;
  }
  if setup.allowed_actions.contains(&ModelSetupAction::Cancel)
    && setup.operation_id.is_some()
    && !matches!(
      setup.phase,
      ModelSetupPhase::Installing | ModelSetupPhase::Activating
    )
  {
    return text.cancel_hint;
  }
  if setup.allowed_actions.contains(&ModelSetupAction::Retry) && setup.managed_model.is_some() {
    return text.retry_hint;
  }
  if setup.allowed_actions.contains(&ModelSetupAction::Download) && setup.managed_model.is_some() {
    return text.download_hint;
  }
  if matches!(
    setup.phase,
    ModelSetupPhase::Required | ModelSetupPhase::Cancelled | ModelSetupPhase::Failed
  ) {
    text.exit_escape_hint
  } else {
    text.exit_hint
  }
}

fn shows_progress(phase: ModelSetupPhase) -> bool {
  matches!(
    phase,
    ModelSetupPhase::WaitingForLock
      | ModelSetupPhase::Checking
      | ModelSetupPhase::Downloading
      | ModelSetupPhase::Verifying
  )
}

fn format_progress(setup: &ModelSetupView, messages: Messages, width: usize) -> String {
  match setup.total_bytes {
    Some(total) if total > 0 => {
      let percent = (u128::from(setup.downloaded_bytes) * 100 / u128::from(total)).min(100);
      let suffix = format!(
        "{percent}% · {} / {}",
        format_bytes(setup.downloaded_bytes),
        format_bytes(total)
      );
      let compact = format!("{percent}% {}", format_bytes(setup.downloaded_bytes));
      if Line::from(suffix.as_str()).width() > width {
        return compact;
      }
      let suffix_width = Line::from(suffix.as_str()).width();
      let Some(bar_width) = width
        .checked_sub(suffix_width.saturating_add(3))
        .filter(|bar_width| *bar_width > 0)
      else {
        return suffix;
      };
      let filled = (bar_width as u128 * percent / 100) as usize;
      format!(
        "[{}{}] {suffix}",
        "#".repeat(filled),
        "-".repeat(bar_width.saturating_sub(filled))
      )
    }
    _ => {
      let suffix = format!(
        "{} · {} {}",
        messages.setup.unknown_total,
        messages.setup.downloaded,
        format_bytes(setup.downloaded_bytes)
      );
      let compact = format!("… {}", format_bytes(setup.downloaded_bytes));
      if Line::from(suffix.as_str()).width() > width {
        return compact;
      }
      let suffix_width = Line::from(suffix.as_str()).width();
      let Some(bar_width) = width
        .checked_sub(suffix_width.saturating_add(3))
        .filter(|bar_width| *bar_width > 0)
      else {
        return suffix;
      };
      let pattern = (0..bar_width)
        .map(|index| if index % 3 == 0 { '~' } else { ' ' })
        .collect::<String>();
      format!("[{pattern}] {suffix}")
    }
  }
}

fn format_progress_line(setup: &ModelSetupView, messages: Messages, inner_width: usize) -> String {
  let label = format!("{}: ", messages.setup.progress);
  let label_width = Line::from(label.as_str()).width();
  let body = format_progress(setup, messages, inner_width.saturating_sub(label_width));
  format!("{label}{body}")
}

fn format_bytes(bytes: u64) -> String {
  const KIB: u64 = 1024;
  const MIB: u64 = KIB * 1024;
  const GIB: u64 = MIB * 1024;
  if bytes < KIB {
    return format!("{bytes} B");
  }
  let (value, unit) = if bytes < MIB {
    (bytes as f64 / KIB as f64, "KiB")
  } else if bytes < GIB {
    (bytes as f64 / MIB as f64, "MiB")
  } else {
    (bytes as f64 / GIB as f64, "GiB")
  };
  format!("{value:.2} {unit}")
}

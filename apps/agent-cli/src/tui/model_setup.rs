use ratatui::{
  Frame,
  layout::{Constraint, Layout},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};

use crate::model_manager::{DownloadPhase, ModelDownloadProgress};

use super::{Locale, Theme};

pub struct ModelSetupView<'a> {
  pub model_id: &'a str,
  pub download_bytes: u64,
  pub progress: ModelDownloadProgress,
  pub awaiting_confirmation: bool,
}

pub fn render_model_setup(
  frame: &mut Frame<'_>,
  view: &ModelSetupView<'_>,
  theme: Theme,
  locale: Locale,
) {
  let area = frame.area();
  frame.render_widget(
    Block::default().style(Style::default().bg(theme.background)),
    area,
  );
  let rows = Layout::vertical([
    Constraint::Length(1),
    Constraint::Length(3),
    Constraint::Length(2),
    Constraint::Length(3),
    Constraint::Min(1),
  ])
  .margin(1)
  .split(area);
  frame.render_widget(
    Paragraph::new(Line::from(vec![
      Span::styled(
        "LYONBROWN",
        Style::default()
          .fg(theme.accent_primary)
          .add_modifier(Modifier::BOLD),
      ),
      Span::styled("  MODEL SETUP", Style::default().fg(theme.text_faint)),
    ])),
    rows[0],
  );

  let size = format_bytes(view.download_bytes);
  let description = match locale {
    Locale::EnUs => format!(
      "Local model  {}\nRecommended default · {size} · download only after confirmation",
      view.model_id
    ),
    Locale::ZhCn => format!(
      "本地模型  {}\n推荐默认模型 · {size} · 仅在确认后下载",
      view.model_id
    ),
  };
  frame.render_widget(
    Paragraph::new(description)
      .style(Style::default().fg(theme.text_primary))
      .wrap(Wrap { trim: true }),
    rows[1],
  );

  let ratio = view.progress.ratio();
  let label = format!(
    "{} / {}  {:.0}%",
    format_bytes(view.progress.downloaded_bytes),
    view
      .progress
      .total_bytes
      .map(format_bytes)
      .unwrap_or_else(|| "—".into()),
    ratio * 100.0
  );
  frame.render_widget(
    Gauge::default()
      .gauge_style(
        Style::default()
          .fg(theme.accent_secondary)
          .bg(theme.surface_raised),
      )
      .ratio(ratio)
      .label(label),
    rows[2],
  );

  let status = status_text(view, locale);
  frame.render_widget(
    Paragraph::new(status)
      .block(
        Block::default()
          .borders(Borders::TOP)
          .border_style(Style::default().fg(theme.border_default)),
      )
      .style(Style::default().fg(theme.text_secondary))
      .wrap(Wrap { trim: true }),
    rows[3],
  );
}

fn status_text(view: &ModelSetupView<'_>, locale: Locale) -> &'static str {
  if view.awaiting_confirmation {
    return match locale {
      Locale::EnUs => "Enter/D  Download     Q/Esc  Exit without downloading",
      Locale::ZhCn => "Enter/D  下载     Q/Esc  不下载并退出",
    };
  }
  match (locale, view.progress.phase) {
    (Locale::ZhCn, DownloadPhase::Checking) => "正在检查本地模型…",
    (Locale::ZhCn, DownloadPhase::Downloading) => "后台下载中 · Esc/C 取消",
    (Locale::ZhCn, DownloadPhase::Verifying) => "正在校验 SHA-256…",
    (Locale::ZhCn, DownloadPhase::Ready) => "模型已就绪",
    (Locale::EnUs, DownloadPhase::Checking) => "Checking local model…",
    (Locale::EnUs, DownloadPhase::Downloading) => "Downloading in background · Esc/C cancel",
    (Locale::EnUs, DownloadPhase::Verifying) => "Verifying SHA-256…",
    (Locale::EnUs, DownloadPhase::Ready) => "Model ready",
  }
}

fn format_bytes(bytes: u64) -> String {
  const MIB: f64 = 1024.0 * 1024.0;
  const GIB: f64 = 1024.0 * MIB;
  if bytes as f64 >= GIB {
    format!("{:.2} GiB", bytes as f64 / GIB)
  } else {
    format!("{:.1} MiB", bytes as f64 / MIB)
  }
}

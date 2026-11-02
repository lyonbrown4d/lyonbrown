use super::{Focus, ProviderDialog};
use crate::{theme::Theme, view_model::safe_text};
use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::Line,
  widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
fn area(screen: Rect) -> Rect {
  let width = screen.width.saturating_sub(2).min(82);
  let height = screen.height.saturating_sub(2).min(21);
  Rect::new(
    screen.x + (screen.width - width) / 2,
    screen.y + (screen.height - height) / 2,
    width,
    height,
  )
}
pub(super) fn credential_area(screen: Rect) -> Option<Rect> {
  let area = area(screen);
  (area.width >= 54 && area.height >= 19)
    .then(|| Rect::new(area.x + 3, area.y + 9, area.width - 6, 1))
}
pub(crate) fn render(frame: &mut Frame, dialog: &ProviderDialog, theme: Theme) {
  let area = area(frame.area());
  let base = Style::default().fg(theme.text).bg(theme.background);
  frame.render_widget(Clear, area);
  frame.render_widget(
    Block::default()
      .title(" Provider 凭据 ")
      .borders(Borders::ALL)
      .style(base)
      .border_style(Style::default().fg(theme.accent)),
    area,
  );
  if credential_area(frame.area()).is_none() {
    frame.render_widget(
      Paragraph::new("请扩大终端以配置 Provider。\nEsc 关闭")
        .style(base)
        .wrap(Wrap { trim: false }),
      area.inner(ratatui::layout::Margin::new(1, 1)),
    );
    return;
  }
  let content = Rect::new(area.x + 3, area.y + 2, area.width - 6, 5);
  let state = match dialog.provider.credential_state.as_str() {
    "configured" => "daemon 已保存",
    "not_required" => "不需要密钥",
    "detected" => "daemon 环境已检测到",
    _ => "尚未保存",
  };
  frame.render_widget(
    Paragraph::new(vec![
      Line::styled(
        safe_text(&dialog.provider.name),
        Style::default().add_modifier(Modifier::BOLD),
      ),
      Line::from(format!(
        "协议  {}",
        safe_text(&dialog.provider.protocols.join(" / "))
      )),
      Line::from(format!("凭据  {state}（不会回显已有密钥）")),
      Line::from(""),
      Line::from("如需自定义地址和模型，请返回列表选择「新增 Provider」。"),
    ])
    .style(base)
    .wrap(Wrap { trim: false }),
    content,
  );
  let input = Rect::new(area.x + 2, area.y + 8, area.width - 4, 3);
  frame.render_widget(
    Block::default()
      .title(if dialog.editable() {
        " 新密钥 · 留空保留现有配置 "
      } else {
        " 此提供方不支持密钥配置 "
      })
      .borders(Borders::ALL)
      .border_style(Style::default().fg(if dialog.focus == Focus::Credential {
        theme.accent
      } else {
        theme.muted
      })),
    input,
  );
  let buttons = [
    (Focus::Save, "保存"),
    (Focus::Import, "导入环境变量"),
    (Focus::Remove, "移除凭据"),
    (Focus::Close, "关闭"),
  ];
  let spans = buttons
    .into_iter()
    .map(|(focus, label)| {
      ratatui::text::Span::styled(
        format!(
          " {}{label} ",
          if dialog.focus == focus { "› " } else { "  " }
        ),
        Style::default()
          .fg(if dialog.focus == focus {
            theme.accent
          } else {
            theme.text
          })
          .add_modifier(if dialog.focus == focus {
            Modifier::BOLD
          } else {
            Modifier::empty()
          }),
      )
    })
    .collect::<Vec<_>>();
  frame.render_widget(
    Paragraph::new(Line::from(spans)),
    Rect::new(area.x + 2, area.y + 12, area.width - 4, 2),
  );
  frame.render_widget(
    Paragraph::new(safe_text(&dialog.message))
      .style(Style::default().fg(if dialog.confirm_remove {
        theme.warning
      } else {
        theme.muted
      }))
      .wrap(Wrap { trim: false }),
    Rect::new(area.x + 3, area.y + 14, area.width - 6, 3),
  );
  frame.render_widget(
    Paragraph::new("Tab 切换 · Enter 确认 · Esc 关闭").style(Style::default().fg(theme.muted)),
    Rect::new(area.x + 3, area.y + 18, area.width - 6, 1),
  );
}

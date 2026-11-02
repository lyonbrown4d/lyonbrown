use super::{Focus, ProfileDialog};
use crate::theme::Theme;
use agent_protocol::v1::RemoteProviderProtocol;
use ratatui::{
  Frame,
  layout::Rect,
  style::Style,
  widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

fn area(screen: Rect) -> Option<Rect> {
  if screen.width < 32 || screen.height < 20 {
    return None;
  }
  let width = screen.width.min(90);
  let height = 20;
  Some(Rect::new(
    screen.x + (screen.width - width) / 2,
    screen.y + (screen.height - height) / 2,
    width,
    height,
  ))
}
pub(super) fn field_area(screen: Rect, index: u16) -> Option<Rect> {
  area(screen).map(|area| Rect::new(area.x + 2, area.y + 2 + index * 2, area.width - 4, 1))
}
pub(crate) fn render(frame: &mut Frame, dialog: &ProfileDialog, theme: Theme) {
  let Some(area) = area(frame.area()) else {
    frame.render_widget(Clear, frame.area());
    frame.render_widget(
      Paragraph::new("窗口过小；至少需要 32×20。Esc 关闭配置。"),
      frame.area(),
    );
    return;
  };
  frame.render_widget(Clear, area);
  frame.render_widget(
    Block::default()
      .title("自定义 Provider")
      .borders(Borders::ALL)
      .style(Style::default().bg(theme.background).fg(theme.text)),
    area,
  );
  let labels = [
    ("名称", Focus::Name),
    ("协议 ←/→/空格", Focus::Protocol),
    ("Base URL", Focus::Url),
    ("模型 ID", Focus::Model),
    ("凭据（留空保留）", Focus::Credential),
  ];
  for (index, (label, focus)) in labels.into_iter().enumerate() {
    frame.render_widget(
      Paragraph::new(label).style(Style::default().fg(if dialog.focus == focus {
        theme.accent
      } else {
        theme.muted
      })),
      Rect::new(area.x + 2, area.y + 1 + index as u16 * 2, area.width - 4, 1),
    );
  }
  for (index, field) in [(0, 0), (1, 2), (2, 3)] {
    if let Some(field_area) = field_area(frame.area(), field) {
      let mut editor = dialog.editors[index].clone();
      editor.set_style(Style::default().fg(theme.text).bg(theme.background));
      editor.set_cursor_line_style(Style::default());
      if dialog.focus.editor() != Some(index) || dialog.busy {
        editor.set_cursor_style(Style::default());
      }
      frame.render_widget(&editor, field_area);
    }
  }
  let protocol = match RemoteProviderProtocol::try_from(dialog.profile.protocol).ok() {
    Some(RemoteProviderProtocol::OpenaiResponses) => "OpenAI Responses",
    Some(RemoteProviderProtocol::OpenaiCompletions) => "OpenAI Chat Completions",
    Some(RemoteProviderProtocol::AnthropicMessages) => "Anthropic Messages",
    _ => "请选择协议",
  };
  if let Some(field) = field_area(frame.area(), 1) {
    frame.render_widget(Paragraph::new(protocol), field);
  }
  let buttons = [
    (Focus::Probe, "探测"),
    (Focus::Save, "保存"),
    (Focus::Remove, "移除"),
    (Focus::Close, "关闭"),
  ]
  .map(|(focus, name)| {
    if dialog.focus == focus {
      format!("[{name}]")
    } else {
      name.into()
    }
  })
  .join("   ");
  frame.render_widget(
    Paragraph::new(buttons).style(Style::default().fg(theme.accent)),
    Rect::new(area.x + 2, area.y + 12, area.width - 4, 1),
  );
  frame.render_widget(
    Paragraph::new(dialog.message.as_str()).wrap(Wrap { trim: false }),
    Rect::new(area.x + 2, area.y + 14, area.width - 4, 3),
  );
  frame.render_widget(
    Paragraph::new("Tab 切换 · Enter 执行 · Esc 关闭（已受理操作仍可能完成）")
      .wrap(Wrap { trim: false })
      .style(Style::default().fg(theme.muted)),
    Rect::new(area.x + 2, area.y + 17, area.width - 4, 2),
  );
}

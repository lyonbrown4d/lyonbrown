//! Pure presentation projection. Daemon actions and authority stay in input/update.
use ratatui::text::{Line, Text};

use crate::{
  composer::Composer,
  model::{App, Connection, ContextPanel},
  theme::Theme,
};

pub struct ApprovalView {
  pub description: String,
  pub pending: usize,
}

pub struct TaskListView {
  pub items: Vec<String>,
  pub selected: usize,
}

pub struct CommandPaletteView {
  pub items: Vec<(String, String)>,
  pub selected: usize,
}

pub struct ViewModel<'a> {
  pub connection: &'static str,
  pub connected: bool,
  pub title: String,
  pub transcript: Text<'static>,
  pub compact_transcript: Text<'static>,
  pub approval: Option<ApprovalView>,
  pub context: Option<(&'static str, Text<'static>)>,
  pub task_list: Option<TaskListView>,
  pub composer: &'a Composer,
  pub submitting: bool,
  pub active: bool,
  pub notice: String,
  pub scroll: u16,
  pub context_scroll: u16,
  pub command_palette: Option<CommandPaletteView>,
}

impl<'a> ViewModel<'a> {
  pub fn new(app: &'a App, theme: Theme) -> Self {
    let connection = match app.connection {
      Connection::Connected => "agentd 已连接 · Connected",
      Connection::Connecting => "agentd 连接中 · Connecting…",
      Connection::Disconnected(_) => "agentd 已断开 · Disconnected · Ctrl+R 重连",
    };
    let mut lines = Vec::new();
    if app.messages.is_empty() && app.streaming.is_none() {
      lines.extend([
        Line::from(""),
        Line::from("从一个具体问题开始。"),
        Line::from(""),
        Line::from("会话与任务保存在 agentd。"),
        Line::from("关闭终端不会取消正在进行的任务。"),
        Line::from(""),
        Line::from("可以这样开始"),
        Line::from("❯ 先读取 Cargo.toml，帮我梳理工作区结构。"),
      ]);
    }
    for message in &app.messages {
      let role = match message.role.as_str() {
        "You" => "❯ 你",
        "Error" => "! 错误",
        _ => "● Lyonbrown",
      };
      lines.push(Line::from(role));
      let content = safe_text(&message.content);
      if message.role == "Agent" {
        lines.extend(crate::markdown::render(&content, theme).lines);
      } else {
        lines.extend(content.lines().map(|line| Line::from(line.to_owned())));
      }
      lines.push(Line::from(""));
    }
    if let Some((_, content)) = &app.streaming {
      lines.push(Line::from("● Lyonbrown · 正在回复"));
      lines.extend(crate::markdown::render(&safe_text(content), theme).lines);
    }
    let task_list =
      (app.panel == ContextPanel::Tasks && app.task_detail.is_empty() && !app.task_rows.is_empty())
        .then(|| TaskListView {
          items: app
            .task_rows
            .iter()
            .map(|task| {
              let envelope = task.get("envelope");
              let id = envelope
                .and_then(|value| value.get("task_id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("未知 ID");
              let status = task
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("未知");
              let attempt = task
                .get("attempt_status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("attempt 未知");
              format!(
                "{} · {} · {}",
                safe_text(id),
                safe_text(status),
                safe_text(attempt)
              )
            })
            .collect(),
          selected: app.task_selected,
        });
    let command_palette = (app.panel == ContextPanel::Commands).then(|| {
      let query = app.input.text();
      let query = query.split_whitespace().next().unwrap_or("/");
      let commands = crate::slash_command::filter(query);
      CommandPaletteView {
        items: commands
          .iter()
          .map(|spec| (format!("/{}", spec.name), spec.summary.to_owned()))
          .collect(),
        selected: app.command_selected.min(commands.len().saturating_sub(1)),
      }
    });
    let context = match app.panel {
      ContextPanel::Closed => None,
      ContextPanel::Help => Some((
        "快捷键 · F1 关闭",
        Text::from(format!(
          "Enter 发送 · Alt/Shift+Enter 换行\nPgUp/PgDn 滚动对话或面板\nCtrl+N 新建会话 · Tab 切换会话\nCtrl+R 重连 · Ctrl+X 取消当前 Run\nCtrl+Y 批准一次 · Ctrl+E 拒绝\nCtrl+C / Ctrl+D 退出\nF2 会话 · F3 持久 Task · F4 计算机\n任务面板：↑/↓ 选择 · Enter 查看 · PgDn 下一页\nEsc 关闭面板 · F5 完整审批内容\n\n/draft list · /draft recover SESSION_ID\n/run inspect RUN_ID\n/run abandon RUN_ID --idempotency-key KEY --note '核对记录'\n/run continue RUN_ID --idempotency-key KEY --note '核对记录' --prompt '新的输入'\n恢复保留原 Run 的不确定状态；继续会创建新会话，用 Tab 切换查看。\n/task list --session SESSION [--after CURSOR]\n/task get TASK_ID\n/task dispatch --node-id NODE --node-endpoint https://worker --session SESSION --parent-run RUN --task-id UUID --attempt-id UUID --idempotency-key KEY --instructions '…' --capability workspace.read\n/task cancel TASK_ID --node-id NODE --node-endpoint https://worker\n/task reconcile TASK_ID --node-id NODE --node-endpoint https://worker\n/task accept TASK_ID --node-id NODE --node-endpoint https://worker --accepted true|false\n\n{}\n\n管理命令由 CLI 本地解析，格式错误不会作为提示发送给模型。",
          crate::slash_command::help_text(None)
        )),
      )),
      ContextPanel::Sessions => Some((
        "会话 · Tab 切换",
        Text::from(
          app
            .sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
              Line::from(format!(
                "{} {}",
                if index == app.sessions_cursor {
                  "›"
                } else {
                  " "
                },
                safe_text(&session.title)
              ))
            })
            .collect::<Vec<_>>(),
        ),
      )),
      ContextPanel::Tasks => Some((
        "持久任务 · ↑/↓ 选择 · Enter 查看 · PgDn 下一页",
        Text::from(if task_list.is_some() {
          String::new()
        } else if !app.task_detail.is_empty() {
          safe_text(&app.task_detail)
        } else {
          "当前会话没有已返回的持久任务。输入 /task list 查看。Run 异常使用 /run inspect RUN_ID 核对，保留原始身份。".to_owned()
        }),
      )),
      ContextPanel::Drafts => Some((
        "草稿 · /draft recover SESSION_ID",
        Text::from(if app.draft_detail.is_empty() {
          "使用 /draft list 查看当前及原会话草稿。恢复只填入编辑器；必须由你检查后按 Enter 才会提交。".into()
        } else {
          safe_text(&app.draft_detail)
        }),
      )),
      ContextPanel::Commands => Some((
        "命令 · ↑/↓ 选择 · Tab 补全 · Enter 补全 · Esc 关闭",
        Text::from("输入命令名称以筛选。所有 / 命令均由 CLI 本地解析。"),
      )),
      ContextPanel::Resource => Some((
        "资源详情 · Esc 关闭",
        Text::from(safe_text(&app.resource_detail)),
      )),
      ContextPanel::Approval => Some((
        "审批详情 · PgUp/PgDn 滚动",
        Text::from(
          app
            .approvals
            .first()
            .map(|approval| safe_text(&approval.description))
            .unwrap_or_else(|| "没有待处理的审批".into()),
        ),
      )),
      ContextPanel::Computer => Some((
        "计算机 · Alt+PgUp/PgDn",
        Text::from(format!(
          "{}\n\n{}\n\n/computer targets · attach · detach · status",
          computer_status(
            &app.computer,
            app.selected_session().map(|session| session.id.as_str())
          ),
          safe_text(&app.computer_detail)
        )),
      )),
    };
    let compact_transcript = if let Some((_, content)) = &app.streaming {
      crate::markdown::render(&safe_text(content), theme)
    } else if let Some(message) = app.messages.last() {
      crate::markdown::render(&safe_text(&message.content), theme)
    } else {
      Text::from("从一个具体问题开始。\nF1 帮助 · Ctrl+N 新建会话")
    };
    let notice = if app.persistence_warning.is_empty() {
      app.notice.clone()
    } else if app.notice.is_empty() {
      app.persistence_warning.clone()
    } else {
      format!("{} · {}", app.notice, app.persistence_warning)
    };
    Self {
      connection,
      connected: app.connection == Connection::Connected,
      title: app
        .selected_session()
        .map(|session| safe_text(&session.title))
        .unwrap_or_else(|| "新会话".into()),
      transcript: Text::from(lines),
      compact_transcript,
      approval: app.approvals.first().map(|approval| ApprovalView {
        description: safe_text(&approval.description),
        pending: app.approvals.len(),
      }),
      context,
      task_list,
      composer: &app.input,
      submitting: app.submitting,
      active: app.active_run.is_some(),
      notice: safe_text(&notice),
      scroll: app.scroll,
      context_scroll: app.computer_scroll,
      command_palette,
    }
  }
}

/// Never forward terminal controls or bidi overrides from model/tool/native output.
pub fn safe_text(value: &str) -> String {
  value
    .chars()
    .filter(|ch| {
      (!ch.is_control() || matches!(ch, '\n' | '\t'))
        && !matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    })
    .collect()
}

fn computer_status(status: &crate::computer::ComputerStatus, selected: Option<&str>) -> String {
  use crate::computer::{ComputerStatus, scope_text};
  match status {
    ComputerStatus::Unavailable => "计算机控制：此构建不可用".into(),
    ComputerStatus::Detached => "计算机控制：未绑定".into(),
    ComputerStatus::Attaching { scope, session } => {
      format!(
        "计算机控制：正在绑定 {} · {}",
        scope_text(scope),
        owner_label(session, selected)
      )
    }
    ComputerStatus::Attached { scope, session } => format!(
      "计算机控制：已绑定 {} · {}",
      scope_text(scope),
      owner_label(session, selected)
    ),
    ComputerStatus::Stopped { reason, .. } => format!("计算机控制：{reason}"),
  }
}

fn owner_label(owner: &str, selected: Option<&str>) -> &'static str {
  if selected == Some(owner) {
    "属于当前会话"
  } else {
    "属于另一会话；切换不会转移控制权"
  }
}

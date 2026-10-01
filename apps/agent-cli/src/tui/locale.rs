#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
  #[default]
  EnUs,
  ZhCn,
}

impl Locale {
  pub const fn id(self) -> &'static str {
    match self {
      Self::EnUs => "en-US",
      Self::ZhCn => "zh-CN",
    }
  }

  pub fn from_id(id: &str) -> Self {
    match id {
      "zh-CN" => Self::ZhCn,
      _ => Self::EnUs,
    }
  }

  pub(crate) const fn messages(self) -> Messages {
    match self {
      Self::EnUs => EN_US,
      Self::ZhCn => ZH_CN,
    }
  }
}

#[derive(Clone, Copy)]
pub(crate) struct Messages {
  pub activity: &'static str,
  pub following: &'static str,
  pub from_bottom: &'static str,
  pub scroll: &'static str,
  pub user: &'static str,
  pub assistant: &'static str,
  pub system: &'static str,
  pub message: &'static str,
  pub ready: &'static str,
  pub running: &'static str,
  pub ready_hint: &'static str,
  pub running_hint: &'static str,
  pub queued_next: &'static str,
  pub workspace: &'static str,
  pub auto: &'static str,
  pub local: &'static str,
  pub details: &'static str,
  pub commands: &'static str,
  pub cancel: &'static str,
  pub approval_required: &'static str,
  pub tool: &'static str,
  pub risk: &'static str,
  pub action: &'static str,
  pub target: &'static str,
  pub impact: &'static str,
  pub working_directory: &'static str,
  pub sandbox: &'static str,
  pub secret_references: &'static str,
  pub none: &'static str,
  pub approval_hint: &'static str,
  pub terminal_too_small: &'static str,
}

const EN_US: Messages = Messages {
  activity: "ACTIVITY",
  following: "following",
  from_bottom: "from bottom",
  scroll: "↑↓ scroll",
  user: "YOU",
  assistant: "AGENT",
  system: "SYSTEM",
  message: "Message",
  ready: "READY",
  running: "RUNNING",
  ready_hint: "Enter send now · Alt-Enter queue next",
  running_hint: "Enter steer now · Alt-Enter queue next",
  queued_next: "Queued next",
  workspace: "workspace",
  auto: "Auto",
  local: "local",
  details: "details",
  commands: "commands",
  cancel: "cancel",
  approval_required: "APPROVAL REQUIRED",
  tool: "tool",
  risk: "risk",
  action: "action",
  target: "target",
  impact: "impact",
  working_directory: "working directory",
  sandbox: "sandbox",
  secret_references: "secret references",
  none: "none",
  approval_hint: "Enter/Esc deny · y allow · n deny · a allow session · d deny all remaining",
  terminal_too_small: "Terminal too small · resize to at least 30×8",
};

const ZH_CN: Messages = Messages {
  activity: "活动",
  following: "跟随最新",
  from_bottom: "距底部",
  scroll: "↑↓ 滚动",
  user: "你",
  assistant: "助手",
  system: "系统",
  message: "消息",
  ready: "就绪",
  running: "运行中",
  ready_hint: "Enter 立即发送 · Alt-Enter 排队下一步",
  running_hint: "Enter 立即引导 · Alt-Enter 排队下一步",
  queued_next: "下一步队列",
  workspace: "工作区",
  auto: "自动",
  local: "本地",
  details: "详情",
  commands: "命令",
  cancel: "取消",
  approval_required: "需要审批",
  tool: "工具",
  risk: "风险",
  action: "动作",
  target: "目标",
  impact: "影响",
  working_directory: "工作目录",
  sandbox: "沙箱",
  secret_references: "Secret 引用",
  none: "无",
  approval_hint: "Enter/Esc 拒绝 · y 允许 · n 拒绝 · a 会话内允许 · d 拒绝后续全部",
  terminal_too_small: "终端过小 · 请调整到至少 30×8",
};

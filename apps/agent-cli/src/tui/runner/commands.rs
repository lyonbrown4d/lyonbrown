use agent_app::ApprovalDecision as AppApprovalDecision;
use agent_tool::ApprovalDecision;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn can_accept_input(task_active: bool, kernel_available: bool) -> bool {
  let _ = task_active;
  kernel_available
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerIntent {
  StartTurn,
  SteerCurrentTurn,
  QueueFollowUp,
}

pub fn composer_intent(task_active: bool, modifiers: KeyModifiers) -> ComposerIntent {
  if task_active && modifiers.contains(KeyModifiers::ALT) {
    ComposerIntent::QueueFollowUp
  } else if task_active {
    ComposerIntent::SteerCurrentTurn
  } else {
    ComposerIntent::StartTurn
  }
}

pub fn quit_key(key: KeyEvent) -> bool {
  key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL)
}

pub fn control_c_key(key: KeyEvent) -> bool {
  key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

pub fn approval_interrupt_key(key: KeyEvent) -> bool {
  key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL
}

pub fn approval_decision(key: KeyEvent) -> Option<ApprovalDecision> {
  if matches!(key.code, KeyCode::Enter | KeyCode::Esc) && key.modifiers == KeyModifiers::NONE {
    return Some(ApprovalDecision::Deny);
  }
  if key.modifiers != KeyModifiers::NONE && key.modifiers != KeyModifiers::SHIFT {
    return None;
  }
  let KeyCode::Char(character) = key.code else {
    return None;
  };
  match character.to_ascii_lowercase() {
    'y' => Some(ApprovalDecision::Allow),
    'n' => Some(ApprovalDecision::Deny),
    'a' => Some(ApprovalDecision::AllowSession),
    'd' => Some(ApprovalDecision::DenyAll),
    _ => None,
  }
}

pub(super) fn map_approval_decision(decision: ApprovalDecision) -> AppApprovalDecision {
  match decision {
    ApprovalDecision::Allow => AppApprovalDecision::AllowOnce,
    ApprovalDecision::AllowSession => AppApprovalDecision::AllowForSession,
    ApprovalDecision::Deny => AppApprovalDecision::DenyOnce,
    ApprovalDecision::DenyAll => AppApprovalDecision::DenyRemaining,
  }
}

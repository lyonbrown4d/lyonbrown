use std::io::{self, IsTerminal, Write};
use std::sync::Arc;

use agent_core::{RiskLevel, SandboxStatus, ToolPreview};
use agent_tool::{
  ApprovalDecision, ApprovalPrompt, ApprovalRequest, ApprovalResponse, SessionApprovalPolicy,
};
use async_trait::async_trait;

use crate::config::ApprovalConfig;

pub struct ConsoleApprovalPrompt;

#[async_trait]
impl ApprovalPrompt for ConsoleApprovalPrompt {
  async fn prompt(&self, request: ApprovalRequest) -> Option<ApprovalResponse> {
    let request_id = request.request_id;
    let request = format_approval_request(&request.preview);
    tokio::task::spawn_blocking(move || read_decision(&request))
      .await
      .ok()
      .flatten()
      .map(|decision| ApprovalResponse {
        request_id,
        decision,
      })
  }
}

fn read_decision(request: &str) -> Option<ApprovalDecision> {
  let mut stderr = io::stderr().lock();
  loop {
    if writeln!(stderr, "{request}").is_err()
      || write!(
        stderr,
        "approve? [y]es / [n]o / [a]lways-this-session / [d]eny-all-rest: "
      )
      .is_err()
      || stderr.flush().is_err()
    {
      return None;
    }
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
      return None;
    }
    if let Some(decision) = parse_approval_decision(&answer) {
      return Some(decision);
    }
  }
}

pub fn policy_from_config(config: &ApprovalConfig) -> SessionApprovalPolicy {
  if config.approve_all {
    SessionApprovalPolicy::approve_all()
  } else if config.deny_all {
    SessionApprovalPolicy::deny_all()
  } else if io::stdin().is_terminal() && io::stderr().is_terminal() {
    SessionApprovalPolicy::interactive_with_timeout(
      Arc::new(ConsoleApprovalPrompt),
      std::time::Duration::from_secs(config.timeout_seconds),
    )
  } else {
    SessionApprovalPolicy::non_interactive()
  }
}

pub fn parse_approval_decision(input: &str) -> Option<ApprovalDecision> {
  match input.trim().to_ascii_lowercase().as_str() {
    "y" | "yes" => Some(ApprovalDecision::Allow),
    "n" | "no" => Some(ApprovalDecision::Deny),
    "a" | "always" => Some(ApprovalDecision::AllowSession),
    "d" | "deny-all" => Some(ApprovalDecision::DenyAll),
    _ => None,
  }
}

pub fn format_approval_request(preview: &ToolPreview) -> String {
  let declared_secrets = preview
    .required_secrets
    .iter()
    .map(ToString::to_string)
    .map(|name| crate::tui::approval_safe(&name))
    .collect::<Vec<_>>();
  let sandbox = match preview.sandbox {
    SandboxStatus::WorkspaceRestricted => "workspace-restricted",
    SandboxStatus::HostUnsandboxed => "host/unsandboxed",
    SandboxStatus::Unknown => "unknown",
  };
  let risk = match preview.risk {
    RiskLevel::Read => "READ",
    RiskLevel::Write => "WRITE",
    RiskLevel::Execute | RiskLevel::Network => "HIGH",
  };
  format!(
    "tool: {}\nrisk: {}\naction: {}\ntarget: {}\nimpact: {}\nworking directory: {}\nsandbox: {}\ndeclared secrets: {}",
    crate::tui::approval_safe(&preview.tool),
    risk,
    crate::tui::approval_safe(&preview.action),
    crate::tui::approval_safe(&preview.target),
    crate::tui::approval_safe(&preview.impact),
    preview.working_directory.as_ref().map_or_else(
      || "not applicable".into(),
      |path| crate::tui::approval_safe(&path.display().to_string())
    ),
    sandbox,
    if declared_secrets.is_empty() {
      "none".into()
    } else {
      declared_secrets.join(", ")
    }
  )
}

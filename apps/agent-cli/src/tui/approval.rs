use std::sync::mpsc;

use agent_core::{RiskLevel, ToolDefinition, ToolInvocation};
use agent_tool::{ApprovalDecision, ApprovalPrompt};

use super::terminal_safe;

#[derive(Debug)]
pub struct ApprovalRequest {
  pub tool: String,
  pub risk: RiskLevel,
  pub arguments: String,
  pub secret_references: Vec<String>,
  reply: mpsc::Sender<ApprovalDecision>,
}

impl ApprovalRequest {
  pub fn respond(self, decision: ApprovalDecision) {
    let _ = self.reply.send(decision);
  }
}

pub struct TuiApprovalPrompt {
  requests: mpsc::Sender<ApprovalRequest>,
}

impl TuiApprovalPrompt {
  pub fn channel() -> (Self, mpsc::Receiver<ApprovalRequest>) {
    let (sender, receiver) = mpsc::channel();
    (Self { requests: sender }, receiver)
  }
}

impl ApprovalPrompt for TuiApprovalPrompt {
  fn prompt(&self, definition: &ToolDefinition, invocation: &ToolInvocation) -> ApprovalDecision {
    let (reply, response) = mpsc::channel();
    let request = ApprovalRequest {
      tool: terminal_safe(&definition.name),
      risk: definition.risk,
      arguments: serde_json::to_string_pretty(&invocation.arguments)
        .unwrap_or_else(|_| "<invalid JSON>".into()),
      secret_references: definition
        .requires_secrets
        .iter()
        .map(ToString::to_string)
        .map(|name| terminal_safe(&name))
        .collect(),
      reply,
    };
    if self.requests.send(request).is_err() {
      return ApprovalDecision::Deny;
    }
    response.recv().unwrap_or(ApprovalDecision::Deny)
  }
}

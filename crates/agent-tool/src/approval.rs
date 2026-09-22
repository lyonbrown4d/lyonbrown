use agent_core::{ToolDefinition, ToolInvocation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Allow,
    AllowSession,
    Deny,
}

pub trait ApprovalPolicy: Send + Sync {
    fn approve(&self, definition: &ToolDefinition, invocation: &ToolInvocation)
    -> ApprovalDecision;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAllApproval;

impl ApprovalPolicy for AllowAllApproval {
    fn approve(
        &self,
        _definition: &ToolDefinition,
        _invocation: &ToolInvocation,
    ) -> ApprovalDecision {
        ApprovalDecision::Allow
    }
}

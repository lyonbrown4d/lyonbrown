use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use agent_core::{RiskLevel, ToolDefinition, ToolInvocation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Allow,
    AllowSession,
    Deny,
    DenyAll,
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

pub trait ApprovalPrompt: Send + Sync {
    fn prompt(&self, definition: &ToolDefinition, invocation: &ToolInvocation) -> ApprovalDecision;
}

enum ApprovalMode {
    Interactive(Arc<dyn ApprovalPrompt>),
    ApproveAll,
    DenyAll,
    NonInteractive,
}

#[derive(Default)]
struct SessionState {
    allowed_tools: HashSet<String>,
    deny_all_rest: bool,
}

pub struct SessionApprovalPolicy {
    mode: ApprovalMode,
    state: Mutex<SessionState>,
}

impl SessionApprovalPolicy {
    pub fn interactive(prompt: Arc<dyn ApprovalPrompt>) -> Self {
        Self::new(ApprovalMode::Interactive(prompt))
    }

    pub fn approve_all() -> Self {
        Self::new(ApprovalMode::ApproveAll)
    }

    pub fn deny_all() -> Self {
        Self::new(ApprovalMode::DenyAll)
    }

    pub fn non_interactive() -> Self {
        Self::new(ApprovalMode::NonInteractive)
    }

    fn new(mode: ApprovalMode) -> Self {
        Self {
            mode,
            state: Mutex::new(SessionState::default()),
        }
    }
}

impl ApprovalPolicy for SessionApprovalPolicy {
    fn approve(
        &self,
        definition: &ToolDefinition,
        invocation: &ToolInvocation,
    ) -> ApprovalDecision {
        if definition.risk == RiskLevel::Read {
            return ApprovalDecision::Allow;
        }
        {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.deny_all_rest {
                return ApprovalDecision::Deny;
            }
            if state.allowed_tools.contains(&definition.name) {
                return ApprovalDecision::Allow;
            }
        }
        let decision = match &self.mode {
            ApprovalMode::Interactive(prompt) => prompt.prompt(definition, invocation),
            ApprovalMode::ApproveAll => ApprovalDecision::Allow,
            ApprovalMode::DenyAll | ApprovalMode::NonInteractive => ApprovalDecision::Deny,
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match decision {
            ApprovalDecision::AllowSession => {
                state.allowed_tools.insert(definition.name.clone());
            }
            ApprovalDecision::DenyAll => state.deny_all_rest = true,
            ApprovalDecision::Allow | ApprovalDecision::Deny => {}
        }
        decision
    }
}

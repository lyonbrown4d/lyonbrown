use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use agent_core::{RiskLevel, ToolCallId, ToolDefinition, ToolInvocation, ToolSource};
use agent_tool::{ApprovalDecision, ApprovalPolicy, ApprovalPrompt, SessionApprovalPolicy};
use serde_json::json;

struct ScriptedPrompt {
    decisions: Mutex<VecDeque<ApprovalDecision>>,
}

impl ScriptedPrompt {
    fn new(decisions: impl IntoIterator<Item = ApprovalDecision>) -> Self {
        Self {
            decisions: Mutex::new(decisions.into_iter().collect()),
        }
    }
}

impl ApprovalPrompt for ScriptedPrompt {
    fn prompt(
        &self,
        _definition: &ToolDefinition,
        _invocation: &ToolInvocation,
    ) -> ApprovalDecision {
        self.decisions.lock().unwrap().pop_front().unwrap()
    }
}

fn definition(name: &str, risk: RiskLevel) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: String::new(),
        input_schema: json!({"type": "object"}),
        source: ToolSource::BuiltIn,
        risk,
        requires_secrets: vec![],
    }
}

fn invocation(name: &str) -> ToolInvocation {
    ToolInvocation {
        id: ToolCallId::new(),
        name: name.into(),
        arguments: json!({}),
    }
}

#[test]
fn read_tools_are_always_allowed_without_prompting() {
    let policy = SessionApprovalPolicy::interactive(Arc::new(ScriptedPrompt::new([])));
    assert_eq!(
        policy.approve(
            &definition("read_file", RiskLevel::Read),
            &invocation("read_file")
        ),
        ApprovalDecision::Allow
    );
}

#[test]
fn non_interactive_sessions_fail_closed_for_high_risk_tools() {
    let policy = SessionApprovalPolicy::non_interactive();
    assert_eq!(
        policy.approve(
            &definition("shell", RiskLevel::Execute),
            &invocation("shell")
        ),
        ApprovalDecision::Deny
    );
}

#[test]
fn allow_session_is_remembered_per_tool_name() {
    let prompt = Arc::new(ScriptedPrompt::new([ApprovalDecision::AllowSession]));
    let policy = SessionApprovalPolicy::interactive(prompt);
    let definition = definition("write_file", RiskLevel::Write);

    assert_eq!(
        policy.approve(&definition, &invocation("write_file")),
        ApprovalDecision::AllowSession
    );
    assert_eq!(
        policy.approve(&definition, &invocation("write_file")),
        ApprovalDecision::Allow
    );
}

#[test]
fn deny_all_rest_is_remembered_for_later_high_risk_tools() {
    let prompt = Arc::new(ScriptedPrompt::new([ApprovalDecision::DenyAll]));
    let policy = SessionApprovalPolicy::interactive(prompt);

    assert_eq!(
        policy.approve(
            &definition("shell", RiskLevel::Execute),
            &invocation("shell")
        ),
        ApprovalDecision::DenyAll
    );
    assert_eq!(
        policy.approve(
            &definition("write_file", RiskLevel::Write),
            &invocation("write_file")
        ),
        ApprovalDecision::Deny
    );
}

#[test]
fn command_line_modes_apply_only_to_high_risk_tools() {
    let approve = SessionApprovalPolicy::approve_all();
    let deny = SessionApprovalPolicy::deny_all();

    assert_eq!(
        approve.approve(
            &definition("shell", RiskLevel::Execute),
            &invocation("shell")
        ),
        ApprovalDecision::Allow
    );
    assert_eq!(
        deny.approve(
            &definition("shell", RiskLevel::Execute),
            &invocation("shell")
        ),
        ApprovalDecision::Deny
    );
    assert_eq!(
        deny.approve(
            &definition("read_file", RiskLevel::Read),
            &invocation("read_file")
        ),
        ApprovalDecision::Allow
    );
}

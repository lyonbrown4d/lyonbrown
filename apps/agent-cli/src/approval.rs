use std::io::{self, IsTerminal, Write};
use std::sync::Arc;

use agent_core::{ToolDefinition, ToolInvocation};
use agent_tool::{ApprovalDecision, ApprovalPrompt, SessionApprovalPolicy};

use crate::config::ApprovalConfig;

pub struct ConsoleApprovalPrompt;

impl ApprovalPrompt for ConsoleApprovalPrompt {
    fn prompt(&self, definition: &ToolDefinition, invocation: &ToolInvocation) -> ApprovalDecision {
        let request = format_approval_request(definition, invocation);
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
                return ApprovalDecision::Deny;
            }
            let mut answer = String::new();
            if io::stdin().read_line(&mut answer).is_err() {
                return ApprovalDecision::Deny;
            }
            if let Some(decision) = parse_approval_decision(&answer) {
                return decision;
            }
        }
    }
}

pub fn policy_from_config(config: &ApprovalConfig) -> SessionApprovalPolicy {
    if config.approve_all {
        SessionApprovalPolicy::approve_all()
    } else if config.deny_all {
        SessionApprovalPolicy::deny_all()
    } else if io::stdin().is_terminal() && io::stderr().is_terminal() {
        SessionApprovalPolicy::interactive(Arc::new(ConsoleApprovalPrompt))
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

pub fn format_approval_request(definition: &ToolDefinition, invocation: &ToolInvocation) -> String {
    let arguments = serde_json::to_string_pretty(&invocation.arguments)
        .unwrap_or_else(|_| "<invalid JSON>".into());
    let declared_secrets = definition
        .requires_secrets
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    format!(
        "tool: {}\nrisk: {:?}\narguments:\n{}\ndeclared secrets: {}",
        definition.name,
        definition.risk,
        arguments,
        if declared_secrets.is_empty() {
            "none".into()
        } else {
            declared_secrets.join(", ")
        }
    )
}

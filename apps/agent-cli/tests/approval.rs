use agent_cli::approval::{format_approval_request, parse_approval_decision};
use agent_core::{RiskLevel, SecretRef, ToolCallId, ToolDefinition, ToolInvocation, ToolSource};
use agent_tool::ApprovalDecision;
use serde_json::json;

#[test]
fn approval_keys_map_to_policy_decisions() {
    assert_eq!(parse_approval_decision("y"), Some(ApprovalDecision::Allow));
    assert_eq!(
        parse_approval_decision("yes"),
        Some(ApprovalDecision::Allow)
    );
    assert_eq!(
        parse_approval_decision("a"),
        Some(ApprovalDecision::AllowSession)
    );
    assert_eq!(parse_approval_decision("n"), Some(ApprovalDecision::Deny));
    assert_eq!(
        parse_approval_decision("d"),
        Some(ApprovalDecision::DenyAll)
    );
    assert_eq!(parse_approval_decision("maybe"), None);
}

#[test]
fn approval_text_shows_tool_arguments_and_secret_reference_names() {
    let definition = ToolDefinition {
        name: "shell".into(),
        description: String::new(),
        input_schema: json!({"type": "object"}),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Execute,
        requires_secrets: vec![SecretRef::env("TOKEN")],
    };
    let invocation = ToolInvocation {
        id: ToolCallId::new(),
        name: "shell".into(),
        arguments: json!({
            "command": "echo ready",
            "env": {"TOKEN": "secret:env:TOKEN"}
        }),
    };

    let rendered = format_approval_request(&definition, &invocation);

    assert!(rendered.contains("shell"));
    assert!(rendered.contains("Execute"));
    assert!(rendered.contains("echo ready"));
    assert!(rendered.contains("env:TOKEN"));
    assert!(!rendered.contains("plaintext-secret"));
}

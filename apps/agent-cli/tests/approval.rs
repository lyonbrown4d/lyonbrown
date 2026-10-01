use agent_cli::approval::{format_approval_request, parse_approval_decision};
use agent_core::{RiskLevel, SandboxStatus, SecretRef, ToolPreview};
use agent_tool::ApprovalDecision;

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
  let preview = ToolPreview {
    tool: "shell".into(),
    action: "Run shell command".into(),
    target: "echo ready".into(),
    impact: "Runs directly on the host".into(),
    risk: RiskLevel::Execute,
    sandbox: SandboxStatus::HostUnsandboxed,
    working_directory: Some("F:/Projects/lyonbrown".into()),
    required_secrets: vec![SecretRef::env("TOKEN")],
  };

  let rendered = format_approval_request(&preview);

  assert!(rendered.contains("shell"));
  assert!(rendered.contains("HIGH"));
  assert!(rendered.contains("sandbox: host/unsandboxed"));
  assert!(rendered.contains("echo ready"));
  assert!(rendered.contains("env:TOKEN"));
  assert!(!rendered.contains("plaintext-secret"));
}

#[test]
fn approval_text_escapes_line_and_direction_controls() {
  let preview = ToolPreview {
    tool: "shell".into(),
    action: "Run".into(),
    target: "echo safe\nimpact: forged\u{202e}".into(),
    impact: "host".into(),
    risk: RiskLevel::Execute,
    sandbox: SandboxStatus::HostUnsandboxed,
    working_directory: None,
    required_secrets: vec![],
  };

  let rendered = format_approval_request(&preview);

  assert!(rendered.contains("echo safe\\nimpact: forged�"));
  assert!(!rendered.contains('\u{202e}'));
  assert_eq!(rendered.matches("\nimpact:").count(), 1);
}

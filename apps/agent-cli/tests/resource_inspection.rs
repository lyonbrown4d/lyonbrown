use agent_cli::resource_command::{format_context, format_inventory, format_permissions};
use agent_protocol::v1;

#[test]
fn context_reports_selected_session_scope_without_prompts_or_connection_secrets() {
  let status = v1::RuntimeStatus {
    node: Some(v1::Node {
      id: "node".into(),
      daemon_version: "1.2.3".into(),
      endpoint: "https://user:connection-secret@example.test".into(),
      capabilities: vec!["tasks.manage".into()],
      ..Default::default()
    }),
    session: Some(v1::Session {
      id: "session".into(),
      title: "workspace".into(),
      workspace: "/project".into(),
      owner_node_id: "master".into(),
      owner_epoch: 42,
      ..Default::default()
    }),
    active_run: Some(v1::Run {
      id: "run".into(),
      status: "awaiting_approval".into(),
      prompt: "private-prompt".into(),
      result: "private-result".into(),
      error: "private-error".into(),
      ..Default::default()
    }),
    model: "configured-model".into(),
    provider: "provider".into(),
    ..Default::default()
  };
  let text = format_context(&status);
  for expected in [
    "/project",
    "configured-model",
    "awaiting_approval",
    "master · epoch 42",
    "tasks.manage",
    "does not expose",
  ] {
    assert!(text.contains(expected), "missing {expected}");
  }
  for private in [
    "connection-secret",
    "private-prompt",
    "private-result",
    "private-error",
  ] {
    assert!(!text.contains(private), "exposed {private}");
  }
}

#[test]
fn permissions_show_only_pending_metadata_and_report_unavailable_policy() {
  let approvals = [
    v1::Approval {
      id: "pending\u{1b}[31m".into(),
      tool: "workspace.write\u{202e}".into(),
      arguments_json: "{\"token\":\"secret-argument\"}".into(),
      status: "pending".into(),
      expires_at_ms: 123,
      ..Default::default()
    },
    v1::Approval {
      id: "already-approved".into(),
      status: "approved".into(),
      ..Default::default()
    },
  ];
  let text = format_permissions(&approvals);
  assert!(text.contains("not exposed"));
  assert!(text.contains("pending"));
  assert!(text.contains("expires at 123 ms"));
  assert!(text.contains("active run"));
  assert!(!text.contains("secret-argument"));
  assert!(!text.contains("already-approved"));
  assert!(!text.contains('\u{1b}'));
  assert!(!text.contains('\u{202e}'));
  assert!(format_permissions(&[]).contains("No pending approvals"));
}

#[test]
fn inventory_counts_explicitly_limit_support_claims() {
  for (kind, count) in [("MCP server", 0), ("tool", 7)] {
    let text = format_inventory(kind, count);
    assert!(text.contains(&format!("{kind} count: {count}")));
    assert!(text.contains("only this count"));
    assert!(text.contains("does not establish"));
  }
}

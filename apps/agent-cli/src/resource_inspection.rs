use agent_protocol::v1;
pub fn format_context(status: &v1::RuntimeStatus) -> String {
  let session = status.session.as_ref();
  let capabilities = status
    .node
    .as_ref()
    .map(|node| node.capabilities.join(", "));
  crate::workflow::safe_text(&format!(
    "{}\nSession owner: {} · epoch {}\nDaemon capabilities: {}\nScope: selected session workspace and daemon-reported metadata.\nThe protocol does not expose a full model context, token budget or effective tool scope.",
    super::resource_command::format_runtime_status(status),
    session
      .map(|session| session.owner_node_id.as_str())
      .unwrap_or("unknown"),
    session
      .map(|session| session.owner_epoch)
      .unwrap_or_default(),
    capabilities
      .as_deref()
      .filter(|value| !value.is_empty())
      .unwrap_or("none reported"),
  ))
}

pub fn format_permissions(approvals: &[v1::Approval]) -> String {
  let mut lines = vec![
    "Effective permission policy is not exposed by the current daemon protocol.".to_owned(),
    "Node capabilities describe supported operations; they do not grant permission.".to_owned(),
    "Pending approvals for the active run (arguments withheld here; inspect in Approvals):"
      .to_owned(),
  ];
  let pending = approvals
    .iter()
    .filter(|approval| approval.status == "pending");
  let mut count = 0;
  for approval in pending {
    count += 1;
    lines.push(format!(
      "{} · {} · expires at {} ms",
      approval.id, approval.tool, approval.expires_at_ms
    ));
  }
  if count == 0 {
    lines.push("No pending approvals returned for the active run.".into());
  }
  crate::workflow::safe_text(&lines.join("\n"))
}

pub fn format_inventory(kind: &str, count: u32) -> String {
  crate::workflow::safe_text(&format!(
    "Daemon-reported {kind} count: {count}\nThe current daemon protocol exposes only this count, not an inventory, schemas, connection health or effective permissions.\nA reported count does not establish that an operation is authorized or a server is connected."
  ))
}

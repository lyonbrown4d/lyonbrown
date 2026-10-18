macro_rules! map_record {
    ($name:ident, $($field:ident),+) => {
        impl From<agent_core::$name> for crate::v1::$name {
            fn from(v: agent_core::$name) -> Self { Self { $($field: v.$field),+ } }
        }
    };
}
map_record!(
  Session,
  id,
  title,
  workspace,
  owner_node_id,
  owner_epoch,
  created_at_ms
);
map_record!(
  Run,
  id,
  session_id,
  status,
  prompt,
  result,
  error,
  created_at_ms
);
map_record!(Event, cursor, run_id, kind, payload_json, created_at_ms);
map_record!(
  Approval,
  id,
  run_id,
  tool,
  arguments_json,
  status,
  expires_at_ms
);
map_record!(
  TaskEnvelope,
  task_id,
  parent_run_id,
  session_id,
  owner_node_id,
  owner_epoch,
  attempt_id,
  instructions,
  capabilities,
  workspace,
  idempotency_key,
  deadline_ms
);
map_record!(
  PeerTrust,
  node_id,
  fingerprint,
  capabilities,
  revoked,
  certificate_pem,
  noise_public_key
);
impl From<agent_core::Task> for crate::v1::Task {
  fn from(v: agent_core::Task) -> Self {
    Self {
      envelope: Some(v.envelope.into()),
      status: v.status,
      attempt_status: v.attempt_status,
      result: v.result,
      error: v.error,
      cursor: v.cursor,
    }
  }
}
impl From<crate::v1::TaskEnvelope> for agent_core::TaskEnvelope {
  fn from(v: crate::v1::TaskEnvelope) -> Self {
    Self {
      task_id: v.task_id,
      parent_run_id: v.parent_run_id,
      session_id: v.session_id,
      owner_node_id: v.owner_node_id,
      owner_epoch: v.owner_epoch,
      attempt_id: v.attempt_id,
      instructions: v.instructions,
      capabilities: v.capabilities,
      workspace: v.workspace,
      idempotency_key: v.idempotency_key,
      deadline_ms: v.deadline_ms,
    }
  }
}

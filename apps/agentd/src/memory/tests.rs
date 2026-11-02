use super::{access::selected_scope, wire};
use agent_memory::MemoryAccess;
use agent_protocol::v1;
use memory_core::{MemoryScope, MemorySource};

#[test]
fn scope_kind_selects_only_server_resolved_ids_and_requires_session_authority() {
  let access = MemoryAccess::new(vec![
    MemoryScope::User("owner-node".into()),
    MemoryScope::Session("local-session".into()),
    MemoryScope::Project("opaque-registry-id".into()),
  ]);
  assert_eq!(
    selected_scope(&access, v1::MemoryScopeKind::Project as i32).expect("project"),
    MemoryScope::Project("opaque-registry-id".into())
  );
  let user_only = MemoryAccess::new(vec![MemoryScope::User("owner-node".into())]);
  assert_eq!(
    selected_scope(&user_only, v1::MemoryScopeKind::Session as i32)
      .expect_err("session required")
      .code(),
    tonic::Code::InvalidArgument
  );
  assert!(selected_scope(&access, 0).is_err());
  assert!(selected_scope(&access, 999).is_err());
}

#[test]
fn user_put_records_explicit_provenance_and_cannot_bypass_forget_scrubbing() {
  let request_id = uuid::Uuid::new_v4().to_string();
  let input = v1::PutMemoryRequest {
    session_id: String::new(),
    id: uuid::Uuid::new_v4().to_string(),
    request_id: request_id.clone(),
    scope_kind: v1::MemoryScopeKind::User as i32,
    kind: v1::MemoryKind::Fact as i32,
    state: v1::MemoryState::Active as i32,
    content: "fixture fact".into(),
    expected_revision: 0,
    expires_at_ms: None,
  };
  let mutation =
    wire::put(input.clone(), MemoryScope::User("server-owner".into())).expect("explicit input");
  assert_eq!(mutation.source, MemorySource::Explicit { request_id });
  assert_eq!(
    mutation.item.scope,
    MemoryScope::User("server-owner".into())
  );
  let mut deleted = input;
  deleted.state = v1::MemoryState::Deleted as i32;
  assert_eq!(
    wire::put(deleted, MemoryScope::User("server-owner".into()))
      .expect_err("must use forget")
      .code(),
    tonic::Code::InvalidArgument
  );
}

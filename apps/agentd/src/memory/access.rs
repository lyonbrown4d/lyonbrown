use crate::Daemon;
use agent_memory::MemoryAccess;
use agent_protocol::v1;
use memory_core::{MemoryRepository, MemoryScope};
use tonic::Status;

impl Daemon {
  /// Resolve authority from the authenticated daemon and persisted local Session.
  /// Scope IDs from model output or client payloads never grant access.
  pub(crate) async fn memory_access(&self, session_id: &str) -> Result<MemoryAccess, Status> {
    if session_id.len() > 128 {
      return Err(Status::invalid_argument("invalid session identity"));
    }
    let mut scopes = vec![MemoryScope::User(self.node_id.clone())];
    if session_id.is_empty() {
      return Ok(MemoryAccess::new(scopes));
    }
    let session = self
      .store
      .session(session_id)
      .await
      .map_err(|_| Status::unavailable("session storage unavailable"))?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id {
      return Err(Status::permission_denied(
        "session is not owned by this daemon",
      ));
    }
    // create_session stores a canonical workspace. Keep that persisted identity:
    // recanonicalizing a moved path or retargeted symlink could transfer a scope.
    let project = self
      .store
      .project_scope(&self.node_id, &session.workspace)
      .await
      .map_err(super::failure)?;
    scopes.push(MemoryScope::Session(session.id));
    scopes.push(project);
    self.events.notify_waiters();
    Ok(MemoryAccess::new(scopes))
  }
}
pub(super) fn selected_scope(access: &MemoryAccess, kind: i32) -> Result<MemoryScope, Status> {
  let selected = v1::MemoryScopeKind::try_from(kind)
    .map_err(|_| Status::invalid_argument("invalid memory scope kind"))?;
  if selected == v1::MemoryScopeKind::Unspecified {
    return Err(Status::invalid_argument("invalid memory scope kind"));
  }
  access
    .scopes()
    .iter()
    .find(|scope| {
      matches!(
        (selected, scope),
        (v1::MemoryScopeKind::User, MemoryScope::User(_))
          | (v1::MemoryScopeKind::Project, MemoryScope::Project(_))
          | (v1::MemoryScopeKind::Session, MemoryScope::Session(_))
      )
    })
    .cloned()
    .ok_or_else(|| Status::invalid_argument("selected memory scope requires an authorized session"))
}

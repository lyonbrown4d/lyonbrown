use agent_computer::{AuthorizedAction, DesktopActionRequest, DesktopError, OperationControl};
use uuid::Uuid;

use crate::{
  engine::{ActionRecord, MAX_IDENTITIES, MAX_PENDING, RetainedPreparation},
  *,
};

impl ComputerRuntime {
  pub(crate) fn prepare(
    &self,
    binding: RunBinding,
    request: DesktopActionRequest,
  ) -> RuntimeResult<Reply> {
    let mut state = self.lock()?;
    Self::check_owner(&state, &binding, &request.lease)?;
    if state.actions.contains_key(&request.action_id) {
      return Err(DesktopError::DuplicateAction.into());
    }
    if state.pending.len() >= MAX_PENDING || state.actions.len() >= MAX_IDENTITIES {
      return Err(RuntimeError::CapacityExceeded);
    }
    let prepared = self.runtime.prepare_action(request)?;
    let description = Preparation {
      binding: binding.clone(),
      preparation_id: Uuid::new_v4(),
      request: prepared.request().clone(),
      content_digest: *prepared.content_digest(),
    };
    state.actions.insert(
      description.request.action_id,
      ActionRecord {
        binding,
        result: None,
      },
    );
    state.pending.insert(
      description.preparation_id,
      RetainedPreparation {
        description: description.clone(),
        prepared,
      },
    );
    Ok(Reply::Prepared(description))
  }

  pub(crate) fn execute(
    &self,
    binding: RunBinding,
    preparation_id: Uuid,
    action_id: Uuid,
    content_digest: [u8; 32],
    approval_id: Uuid,
    control: OperationControl,
  ) -> RuntimeResult<Reply> {
    if approval_id.is_nil() {
      return Err(RuntimeError::InvalidControl);
    }
    let retained = {
      let mut state = self.lock()?;
      let pending = state
        .pending
        .get(&preparation_id)
        .ok_or(RuntimeError::PreparationUnavailable)?;
      if pending.description.binding != binding {
        return Err(RuntimeError::BindingMismatch);
      }
      if pending.description.request.action_id != action_id
        || pending.description.content_digest != content_digest
      {
        return Err(RuntimeError::PreparationMismatch);
      }
      Self::check_owner(&state, &binding, &pending.description.request.lease)?;
      // Consume before refresh or input. Any subsequent error leaves a tombstone.
      state
        .pending
        .remove(&preparation_id)
        .ok_or(RuntimeError::PreparationUnavailable)?
    };
    let refreshed = self
      .runtime
      .refresh_prepared_action(&retained.prepared, control.clone())?;
    let receipt = self
      .runtime
      .execute(AuthorizedAction::new(refreshed, approval_id), control)?;
    let result = ExecutionResult {
      binding,
      preparation_id,
      original_request: retained.description.request,
      content_digest,
      receipt,
    };
    let mut state = self.lock()?;
    let record = state
      .actions
      .get_mut(&action_id)
      .ok_or(RuntimeError::CoordinationUnavailable)?;
    record.result = Some(result.clone());
    Ok(Reply::Executed(result))
  }
}

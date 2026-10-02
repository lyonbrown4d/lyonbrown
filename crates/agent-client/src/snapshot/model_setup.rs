use agent_app::{
  ManagedModelView, ModelSetupAction as AppAction, ModelSetupFailureView,
  ModelSetupPhase as AppPhase, ModelSetupSource as AppSource, ModelSetupView,
};
use agent_protocol::v1::{ModelSetupAction, ModelSetupPhase, ModelSetupSnapshot, ModelSetupSource};

use super::SnapshotDecodeError;

pub(super) fn model_setup_from_wire(
  wire: ModelSetupSnapshot,
) -> Result<ModelSetupView, SnapshotDecodeError> {
  let source = source_from_wire(wire.source)?;
  let phase = phase_from_wire(wire.phase)?;
  let allowed_actions = wire
    .allowed_actions
    .iter()
    .copied()
    .map(action_from_wire)
    .collect::<Result<Vec<_>, _>>()?;
  validate_model_setup(&wire, source, phase, &allowed_actions)?;
  let managed_model = wire.managed_model.map(|model| ManagedModelView {
    id: model.id,
    expected_download_bytes: model.expected_download_bytes,
    max_bytes: model.max_bytes,
    min_available_memory_bytes: model.min_available_memory_bytes,
    sha256: model.sha256,
  });
  Ok(ModelSetupView {
    source,
    phase,
    managed_model,
    operation_id: wire.operation_id,
    downloaded_bytes: wire.downloaded_bytes,
    total_bytes: wire.total_bytes,
    failure: wire.failure.map(|failure| ModelSetupFailureView {
      code: failure.code,
      message: failure.message,
      retryable: failure.retryable,
    }),
    allowed_actions,
  })
}

fn validate_model_setup(
  wire: &ModelSetupSnapshot,
  source: AppSource,
  phase: AppPhase,
  allowed_actions: &[AppAction],
) -> Result<(), SnapshotDecodeError> {
  let managed = wire.managed_model.as_ref();
  if source == AppSource::ManagedPreset && managed.is_none() {
    return Err(SnapshotDecodeError::Missing("model_setup.managed_model"));
  }
  if source == AppSource::ConfiguredPath && managed.is_some() {
    return invalid("configured path must not include managed model metadata");
  }
  if let Some(model) = managed {
    if model.id.trim().is_empty() || model.id.len() > 128 {
      return invalid("managed model id is required and must not exceed 128 bytes");
    }
    if model.sha256.len() != 64 || !model.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
      return invalid("managed model sha256 must be 64 ASCII hexadecimal characters");
    }
    if model.max_bytes == 0 || model.expected_download_bytes > model.max_bytes {
      return invalid("managed model size limits are invalid");
    }
    if wire.downloaded_bytes > model.max_bytes
      || wire
        .total_bytes
        .is_some_and(|total| total > model.max_bytes)
    {
      return invalid("model setup progress exceeds managed model max_bytes");
    }
  }
  if wire
    .total_bytes
    .is_some_and(|total| wire.downloaded_bytes > total)
  {
    return invalid("downloaded_bytes exceeds total_bytes");
  }
  if let Some(operation) = wire.operation_id.as_deref()
    && (operation.trim().is_empty() || operation.len() > 128)
  {
    return invalid("operation_id must not be empty or exceed 128 bytes");
  }
  if requires_operation(phase) && wire.operation_id.is_none() {
    return invalid("active model setup requires operation_id");
  }
  if phase == AppPhase::Failed && wire.failure.is_none() {
    return invalid("failed model setup requires failure details");
  }
  if phase != AppPhase::Failed && wire.failure.is_some() {
    return invalid("failure details are only valid in failed phase");
  }
  if allowed_actions != expected_actions(source, phase, wire) {
    return invalid("allowed_actions do not match model setup state");
  }
  Ok(())
}

fn requires_operation(phase: AppPhase) -> bool {
  matches!(
    phase,
    AppPhase::WaitingForLock
      | AppPhase::Checking
      | AppPhase::Downloading
      | AppPhase::Verifying
      | AppPhase::Installing
      | AppPhase::Activating
      | AppPhase::Cancelled
      | AppPhase::Failed
  )
}

fn expected_actions(
  source: AppSource,
  phase: AppPhase,
  wire: &ModelSetupSnapshot,
) -> &'static [AppAction] {
  const DOWNLOAD: &[AppAction] = &[AppAction::Download];
  const CANCEL: &[AppAction] = &[AppAction::Cancel];
  const RETRY: &[AppAction] = &[AppAction::Retry];
  match phase {
    AppPhase::Required if source == AppSource::ManagedPreset => DOWNLOAD,
    AppPhase::WaitingForLock | AppPhase::Checking | AppPhase::Downloading | AppPhase::Verifying => {
      CANCEL
    }
    AppPhase::Cancelled if source == AppSource::ManagedPreset => RETRY,
    AppPhase::Failed
      if source == AppSource::ManagedPreset
        && wire
          .failure
          .as_ref()
          .is_some_and(|failure| failure.retryable) =>
    {
      RETRY
    }
    AppPhase::Required
    | AppPhase::Installing
    | AppPhase::Activating
    | AppPhase::Ready
    | AppPhase::Cancelled
    | AppPhase::Failed => &[],
  }
}

fn invalid<T>(message: &'static str) -> Result<T, SnapshotDecodeError> {
  Err(SnapshotDecodeError::InvalidModelSetup(message))
}

fn source_from_wire(value: i32) -> Result<AppSource, SnapshotDecodeError> {
  match ModelSetupSource::try_from(value) {
    Ok(ModelSetupSource::ManagedPreset) => Ok(AppSource::ManagedPreset),
    Ok(ModelSetupSource::ConfiguredPath) => Ok(AppSource::ConfiguredPath),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "model_setup.source",
      value,
    }),
  }
}

fn phase_from_wire(value: i32) -> Result<AppPhase, SnapshotDecodeError> {
  match ModelSetupPhase::try_from(value) {
    Ok(ModelSetupPhase::Required) => Ok(AppPhase::Required),
    Ok(ModelSetupPhase::WaitingForLock) => Ok(AppPhase::WaitingForLock),
    Ok(ModelSetupPhase::Checking) => Ok(AppPhase::Checking),
    Ok(ModelSetupPhase::Downloading) => Ok(AppPhase::Downloading),
    Ok(ModelSetupPhase::Verifying) => Ok(AppPhase::Verifying),
    Ok(ModelSetupPhase::Installing) => Ok(AppPhase::Installing),
    Ok(ModelSetupPhase::Activating) => Ok(AppPhase::Activating),
    Ok(ModelSetupPhase::Ready) => Ok(AppPhase::Ready),
    Ok(ModelSetupPhase::Cancelled) => Ok(AppPhase::Cancelled),
    Ok(ModelSetupPhase::Failed) => Ok(AppPhase::Failed),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "model_setup.phase",
      value,
    }),
  }
}

fn action_from_wire(value: i32) -> Result<AppAction, SnapshotDecodeError> {
  match ModelSetupAction::try_from(value) {
    Ok(ModelSetupAction::Download) => Ok(AppAction::Download),
    Ok(ModelSetupAction::Cancel) => Ok(AppAction::Cancel),
    Ok(ModelSetupAction::Retry) => Ok(AppAction::Retry),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "model_setup.allowed_actions",
      value,
    }),
  }
}

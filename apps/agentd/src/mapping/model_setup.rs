use agent_app::{
  ModelSetupAction as AppAction, ModelSetupPhase as AppPhase, ModelSetupSource as AppSource,
  ModelSetupView,
};
use agent_protocol::v1::{
  ManagedModel, ModelSetupAction, ModelSetupFailure, ModelSetupPhase, ModelSetupSnapshot,
  ModelSetupSource,
};

pub(super) fn model_setup_to_wire(view: ModelSetupView) -> ModelSetupSnapshot {
  ModelSetupSnapshot {
    source: source_to_wire(view.source) as i32,
    phase: phase_to_wire(view.phase) as i32,
    managed_model: view.managed_model.map(|model| ManagedModel {
      id: model.id,
      expected_download_bytes: model.expected_download_bytes,
      max_bytes: model.max_bytes,
      min_available_memory_bytes: model.min_available_memory_bytes,
      sha256: model.sha256,
    }),
    operation_id: view.operation_id,
    downloaded_bytes: view.downloaded_bytes,
    total_bytes: view.total_bytes,
    failure: view.failure.map(|failure| ModelSetupFailure {
      code: failure.code,
      message: failure.message,
      retryable: failure.retryable,
    }),
    allowed_actions: view
      .allowed_actions
      .into_iter()
      .map(|action| action_to_wire(action) as i32)
      .collect(),
  }
}

fn source_to_wire(source: AppSource) -> ModelSetupSource {
  match source {
    AppSource::ManagedPreset => ModelSetupSource::ManagedPreset,
    AppSource::ConfiguredPath => ModelSetupSource::ConfiguredPath,
  }
}

fn phase_to_wire(phase: AppPhase) -> ModelSetupPhase {
  match phase {
    AppPhase::Required => ModelSetupPhase::Required,
    AppPhase::WaitingForLock => ModelSetupPhase::WaitingForLock,
    AppPhase::Checking => ModelSetupPhase::Checking,
    AppPhase::Downloading => ModelSetupPhase::Downloading,
    AppPhase::Verifying => ModelSetupPhase::Verifying,
    AppPhase::Installing => ModelSetupPhase::Installing,
    AppPhase::Activating => ModelSetupPhase::Activating,
    AppPhase::Ready => ModelSetupPhase::Ready,
    AppPhase::Cancelled => ModelSetupPhase::Cancelled,
    AppPhase::Failed => ModelSetupPhase::Failed,
  }
}

fn action_to_wire(action: AppAction) -> ModelSetupAction {
  match action {
    AppAction::Download => ModelSetupAction::Download,
    AppAction::Cancel => ModelSetupAction::Cancel,
    AppAction::Retry => ModelSetupAction::Retry,
  }
}

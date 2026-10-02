use agent_app::{ModelSetupAction, ModelSetupFailureView, ModelSetupPhase};

use super::ModelSetupCoordinator;
use super::plan::{SetupTarget, managed_view};

impl ModelSetupCoordinator {
  /// Confirm that the prepared replacement became the published daemon epoch.
  pub async fn activation_succeeded(&self, operation_id: &str) -> bool {
    let preset = match self.inner.target {
      SetupTarget::Managed(preset) => preset,
      SetupTarget::Disabled | SetupTarget::Unavailable => return false,
    };
    let mut state = self.inner.state.lock().await;
    if !is_current_activation(&state, operation_id) {
      return false;
    }
    state.active = None;
    state.last_successful_activation = Some(operation_id.into());
    self.inner.snapshots.send_replace(Some(managed_view(
      preset,
      ModelSetupPhase::Ready,
      None,
      0,
      None,
      Vec::new(),
    )));
    true
  }

  /// Report a recoverable failure while starting or publishing a replacement epoch.
  pub async fn activation_failed(&self, operation_id: &str, code: &'static str) -> bool {
    let preset = match self.inner.target {
      SetupTarget::Managed(preset) => preset,
      SetupTarget::Disabled | SetupTarget::Unavailable => return false,
    };
    let mut state = self.inner.state.lock().await;
    if !is_current_activation(&state, operation_id) {
      return false;
    }
    state.active = None;
    state.last_successful_activation = None;
    let mut view = managed_view(
      preset,
      ModelSetupPhase::Failed,
      Some(operation_id.into()),
      0,
      None,
      vec![ModelSetupAction::Retry],
    );
    view.failure = Some(ModelSetupFailureView {
      code: code.into(),
      message: "The model could not be activated. Retry the download.".into(),
      retryable: true,
    });
    self.inner.snapshots.send_replace(Some(view));
    true
  }

  /// Compensate a success ACK when the candidate exits before becoming active.
  pub async fn activation_failed_after_success(
    &self,
    operation_id: &str,
    code: &'static str,
  ) -> bool {
    let preset = match self.inner.target {
      SetupTarget::Managed(preset) => preset,
      SetupTarget::Disabled | SetupTarget::Unavailable => return false,
    };
    let mut state = self.inner.state.lock().await;
    if state.active.is_some() || state.last_successful_activation.as_deref() != Some(operation_id) {
      return false;
    }
    state.last_successful_activation = None;
    let mut view = managed_view(
      preset,
      ModelSetupPhase::Failed,
      Some(operation_id.into()),
      0,
      None,
      vec![ModelSetupAction::Retry],
    );
    view.failure = Some(ModelSetupFailureView {
      code: code.into(),
      message: "The model could not be activated. Retry the download.".into(),
      retryable: true,
    });
    self.inner.snapshots.send_replace(Some(view));
    true
  }
}

fn is_current_activation(state: &super::coordinator::State, operation_id: &str) -> bool {
  state
    .active
    .as_ref()
    .is_some_and(|active| active.id == operation_id && active.phase == ModelSetupPhase::Activating)
}

use std::path::PathBuf;

use agent_app::ModelSetupPhase;
use agent_runtime::model_manager::ModelPreset;
use tokio_util::sync::CancellationToken;

use super::ModelSetupCoordinator;
use super::replacement::dispose_prepared;

impl ModelSetupCoordinator {
  pub(super) async fn activate(
    &self,
    preset: &'static ModelPreset,
    operation_id: &str,
    cancellation: &CancellationToken,
    path: PathBuf,
  ) {
    if !self
      .begin_activation(preset, operation_id, cancellation)
      .await
    {
      return;
    }
    match self.inner.activator.activate(path).await {
      Ok(prepared) => {
        let prepared = prepared.for_activation(operation_id);
        if let Err(rejected) = self.inner.replacements.send(prepared) {
          dispose_prepared(rejected.0).await;
          self
            .finish_failed(preset, operation_id, "activation_unavailable")
            .await;
        }
      }
      Err(_error) => {
        tracing::error!(
          operation_id,
          model_id = preset.id,
          error_code = "activation_failed",
          "managed model activation failed"
        );
        self
          .finish_failed(preset, operation_id, "activation_failed")
          .await;
      }
    }
  }

  async fn begin_activation(
    &self,
    preset: &'static ModelPreset,
    operation_id: &str,
    cancellation: &CancellationToken,
  ) -> bool {
    let mut state = self.inner.state.lock().await;
    let matches_operation = state
      .active
      .as_ref()
      .is_some_and(|active| active.id == operation_id);
    if !matches_operation {
      return false;
    }
    if cancellation.is_cancelled() {
      state.active = None;
      self.publish_cancelled(preset, operation_id);
      return false;
    }
    state.active.as_mut().unwrap().phase = ModelSetupPhase::Activating;
    self.publish_progress(preset, operation_id, ModelSetupPhase::Activating, 0, None);
    true
  }
}

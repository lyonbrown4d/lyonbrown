use std::path::PathBuf;

use agent_app::{AppController, RuntimeTask};
use anyhow::Result;
use async_trait::async_trait;

pub struct PreparedRuntime {
  pub controller: AppController,
  pub runtime: RuntimeTask,
  activation_operation_id: Option<String>,
}

impl PreparedRuntime {
  pub fn new(controller: AppController, runtime: RuntimeTask) -> Self {
    Self {
      controller,
      runtime,
      activation_operation_id: None,
    }
  }

  pub fn activation_operation_id(&self) -> Option<&str> {
    self.activation_operation_id.as_deref()
  }

  pub(super) fn for_activation(mut self, operation_id: &str) -> Self {
    self.activation_operation_id = Some(operation_id.into());
    self
  }
}

#[async_trait]
pub trait RuntimeActivator: Send + Sync + 'static {
  async fn activate(&self, installed_model: PathBuf) -> Result<PreparedRuntime>;
}

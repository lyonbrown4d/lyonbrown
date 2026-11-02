//! Safe dialog presentation; credentials belong to the transient input adapter.
mod backend;
mod input;
mod view;

use agent_protocol::v1::Provider;
pub(crate) use backend::execute;
pub use input::CredentialInput;
pub(crate) use view::render;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
  Credential,
  Save,
  Import,
  Remove,
  Close,
}
impl Focus {
  fn next(self, reverse: bool) -> Self {
    let fields = [
      Self::Credential,
      Self::Save,
      Self::Import,
      Self::Remove,
      Self::Close,
    ];
    let index = fields.iter().position(|value| *value == self).unwrap_or(0);
    fields[(index + if reverse { 4 } else { 1 }) % fields.len()]
  }
}
#[derive(Clone, Debug)]
pub struct ProviderDialog {
  pub id: String,
  pub provider: Provider,
  pub environment: String,
  pub focus: Focus,
  pub busy: bool,
  pub confirm_remove: bool,
  pub message: String,
  pub credential_present: bool,
  /// Changes after an acknowledged mutation, so the input adapter discards undo history.
  pub revision: u64,
}
impl ProviderDialog {
  pub fn new(provider: Provider, environment: String) -> Self {
    Self {
      id: uuid::Uuid::now_v7().to_string(),
      provider,
      environment,
      focus: Focus::Credential,
      busy: false,
      confirm_remove: false,
      message: "填写新密钥或从本机环境变量导入；不会读取已有密钥。".into(),
      credential_present: false,
      revision: 0,
    }
  }
  pub fn editable(&self) -> bool {
    self.provider.runtime_supported
      && self.provider.id != "local"
      && agent_config::provider_definition(&self.provider.id)
        .is_some_and(|definition| !definition.credential_env_names.is_empty())
  }
}
/// Command adapter data is never serialized into App, ViewModel, history or drafts.
pub enum CredentialChange {
  Typed(agent_secrets::SecretString),
  Environment(String),
  Remove,
}

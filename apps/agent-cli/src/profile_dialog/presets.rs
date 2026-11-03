use super::ProfileDialog;
use agent_config::{ProviderProtocol, REMOTE_PROVIDER_PRESETS};
use agent_protocol::v1::{ProviderProfile, RemoteProviderProtocol};
use ratatui_textarea::TextArea;
fn protocol(value: ProviderProtocol) -> RemoteProviderProtocol {
  match value {
    ProviderProtocol::AnthropicMessages => RemoteProviderProtocol::AnthropicMessages,
    ProviderProtocol::GeminiGenerateContent => RemoteProviderProtocol::GeminiGenerateContent,
    ProviderProtocol::OpenaiCompletions => RemoteProviderProtocol::OpenaiCompletions,
    _ => RemoteProviderProtocol::OpenaiResponses,
  }
}
pub(super) fn matching(profile: &ProviderProfile) -> Option<usize> {
  REMOTE_PROVIDER_PRESETS.iter().position(|preset| {
    protocol(preset.protocol) as i32 == profile.protocol
      && preset.base_url.trim_end_matches('/') == profile.base_url.trim_end_matches('/')
  })
}
impl ProfileDialog {
  pub(super) fn select_preset(&mut self, reverse: bool) {
    let count = REMOTE_PROVIDER_PRESETS.len();
    let index = self.preset_index.map_or(0, |index| {
      (index + if reverse { count - 1 } else { 1 }) % count
    });
    let preset = &REMOTE_PROVIDER_PRESETS[index];
    self.id = uuid::Uuid::now_v7().to_string();
    self.credential_present = false;
    self.profile = agent_protocol::v1::ProviderProfile {
      id: uuid::Uuid::now_v7().to_string(),
      protocol: protocol(preset.protocol) as i32,
      ..Default::default()
    };
    self.preset_index = Some(index);
    self.editors = [preset.name, preset.base_url, preset.model].map(|value| {
      let mut editor = TextArea::new(vec![value.into()]);
      editor.set_max_histories(0);
      editor
    });
    self.message =
      "已填入提供方预设，可编辑地址与模型；密钥不会发送给新地址，直到选择探测或保存。".into();
  }
}

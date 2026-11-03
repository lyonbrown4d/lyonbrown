//! Safe profile presentation. Secret editors are owned by the transient input adapter.
mod backend;
mod input;
mod presets;
mod view;
use agent_protocol::v1::{ProviderProbe, ProviderProfile};
pub(crate) use backend::execute;
pub use input::ProfileInput;
use ratatui_textarea::TextArea;
pub(crate) use view::render;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
  Name,
  Protocol,
  Url,
  Model,
  Credential,
  Probe,
  Save,
  Remove,
  Close,
}
impl Focus {
  fn next(self, reverse: bool) -> Self {
    let fields = [
      Self::Name,
      Self::Protocol,
      Self::Url,
      Self::Model,
      Self::Credential,
      Self::Probe,
      Self::Save,
      Self::Remove,
      Self::Close,
    ];
    let index = fields.iter().position(|field| *field == self).unwrap_or(0);
    fields[(index + if reverse { 8 } else { 1 }) % 9]
  }
  fn editor(self) -> Option<usize> {
    match self {
      Self::Name => Some(0),
      Self::Url => Some(1),
      Self::Model => Some(2),
      _ => None,
    }
  }
}
#[derive(Clone)]
pub struct ProfileDialog {
  pub id: String,
  pub profile: ProviderProfile,
  pub focus: Focus,
  pub busy: bool,
  pub confirm_remove: bool,
  pub message: String,
  pub credential_present: bool,
  preset_index: Option<usize>,
  editors: [TextArea<'static>; 3],
}
impl std::fmt::Debug for ProfileDialog {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("ProfileDialog")
      .field("id", &self.id)
      .field("profile", &self.snapshot())
      .field("busy", &self.busy)
      .finish()
  }
}
impl ProfileDialog {
  pub fn new(profile: Option<ProviderProfile>) -> Self {
    let profile = profile.unwrap_or_else(|| ProviderProfile {
      id: uuid::Uuid::now_v7().to_string(),
      protocol: agent_protocol::v1::RemoteProviderProtocol::OpenaiResponses as i32,
      ..Default::default()
    });
    let preset_index = presets::matching(&profile);
    let editors = [&profile.name, &profile.base_url, &profile.model].map(|value| {
      let mut editor = TextArea::new(vec![value.clone()]);
      editor.set_max_histories(0);
      editor
    });
    Self {
      id: uuid::Uuid::now_v7().to_string(),
      profile,
      editors,
      focus: Focus::Name,
      busy: false,
      confirm_remove: false,
      message: "填写配置；探测仅请求模型列表。空密钥保留已有凭据。".into(),
      credential_present: false,
      preset_index,
    }
  }
  pub fn snapshot(&self) -> ProviderProfile {
    let mut profile = self.profile.clone();
    profile.name = self.editors[0].lines().join("");
    profile.base_url = self.editors[1].lines().join("");
    profile.model = self.editors[2].lines().join("");
    profile
  }
  fn changed(&mut self) {
    self.confirm_remove = false;
    self.message = "配置已更改，请重新探测或保存。".into();
  }
}
pub enum ProfileOperation {
  Save {
    profile: ProviderProfile,
    credential: agent_secrets::SecretString,
  },
  Probe {
    profile: ProviderProfile,
    credential: agent_secrets::SecretString,
  },
  Remove {
    id: String,
    revision: u64,
  },
}
#[derive(Debug)]
pub enum ProfileResult {
  Saved(ProviderProfile),
  Removed,
  Probed(ProviderProbe),
}

pub fn apply(
  app: &mut crate::model::App,
  generation: u64,
  id: &str,
  result: Result<ProfileResult, String>,
) {
  if app.generation != generation {
    return;
  }
  let Some(dialog) = app.profile_dialog.as_mut().filter(|dialog| dialog.id == id) else {
    return;
  };
  dialog.busy = false;
  dialog.confirm_remove = false;
  match result {
    Ok(ProfileResult::Saved(profile)) => {
      app.provider_profiles.retain(|entry| entry.id != profile.id);
      app.provider_profiles.push(profile.clone());
      dialog.profile = profile;
      dialog.credential_present = false;
      dialog.message = "已保存。请通过 /model 显式选择模型。".into();
    }
    Ok(ProfileResult::Removed) => {
      app
        .provider_profiles
        .retain(|entry| entry.id != dialog.profile.id);
      app.profile_dialog = None;
    }
    Ok(ProfileResult::Probed(probe)) => {
      dialog.message = if probe.reachable {
        format!(
          "连接成功；发现 {} 个模型。探测不保存配置。",
          probe.model_ids.len()
        )
      } else {
        format!(
          "探测未成功：{}",
          probe
            .message
            .chars()
            .filter(|c| !c.is_control())
            .take(240)
            .collect::<String>()
        )
      };
    }
    Err(message) => dialog.message = message,
  }
}

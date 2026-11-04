//! Native profile editing; secret input remains in a transient masked editor.
mod commands;
mod view;
use agent_client::AgentClient;
use agent_protocol::v1;
use gpui_kit::{component::input::InputState, *};
use std::sync::Arc;

pub(super) struct ProfileEditor {
  runtime: Arc<tokio::runtime::Runtime>,
  client: Option<AgentClient>,
  generation: u64,
  pub(super) pending: bool,
  profiles: Vec<v1::ProviderProfile>,
  profile: v1::ProviderProfile,
  name: Entity<InputState>,
  endpoint: Entity<InputState>,
  model: Entity<InputState>,
  credential: Option<Entity<InputState>>,
  message: String,
  reset_inputs: bool,
}
impl ProfileEditor {
  pub(super) fn new(
    runtime: Arc<tokio::runtime::Runtime>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    Self {
      runtime,
      client: None,
      generation: 0,
      pending: false,
      profiles: Vec::new(),
      profile: fresh(),
      name: cx.new(|cx| InputState::new(window, cx).placeholder("Profile name")),
      endpoint: cx
        .new(|cx| InputState::new(window, cx).placeholder("HTTPS endpoint or loopback HTTP")),
      model: cx.new(|cx| InputState::new(window, cx).placeholder("Model identifier")),
      credential: None,
      reset_inputs: false,
      message: "Choose a provider preset, then enter a key. Endpoint and model remain editable."
        .into(),
    }
  }
  pub(super) fn bind(&mut self, client: Option<AgentClient>, cx: &mut Context<Self>) {
    self.generation = self.generation.saturating_add(1);
    self.client = client;
    self.credential = None;
    self.reset_inputs = true;
    self.profiles.clear();
    self.profile = fresh();
    // Connection replacement is blocked while a command is outstanding.
    if self.client.is_some() {
      self.refresh(cx);
    }
    cx.notify();
  }
  fn edit(&mut self, profile: v1::ProviderProfile, window: &mut Window, cx: &mut Context<Self>) {
    self.name.update(cx, |input, cx| {
      input.set_value(profile.name.clone(), window, cx)
    });
    self.endpoint.update(cx, |input, cx| {
      input.set_value(profile.base_url.clone(), window, cx)
    });
    self.model.update(cx, |input, cx| {
      input.set_value(profile.model.clone(), window, cx)
    });
    self.profile = profile;
    self.credential = None;
    self.message =
      "Blank key keeps this profile's saved key. Probe sends it only to the displayed endpoint."
        .into();
    cx.notify();
  }
  fn preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
    let Some(preset) = agent_config::REMOTE_PROVIDER_PRESETS.get(index) else {
      return;
    };
    use agent_config::ProviderProtocol as P;
    use v1::RemoteProviderProtocol as R;
    let protocol = match preset.protocol {
      P::AnthropicMessages => R::AnthropicMessages,
      P::GeminiGenerateContent => R::GeminiGenerateContent,
      P::OpenaiCompletions => R::OpenaiCompletions,
      _ => R::OpenaiResponses,
    };
    self.edit(
      v1::ProviderProfile {
        name: preset.name.into(),
        base_url: preset.base_url.into(),
        model: preset.model.into(),
        protocol: protocol as i32,
        ..fresh()
      },
      window,
      cx,
    );
    self.message = "New profile selected. Enter its key before probing or saving.".into();
  }
  fn snapshot(&self, cx: &App) -> v1::ProviderProfile {
    v1::ProviderProfile {
      name: self.name.read(cx).value().to_string(),
      base_url: self.endpoint.read(cx).value().to_string(),
      model: self.model.read(cx).value().to_string(),
      ..self.profile.clone()
    }
  }
}
fn fresh() -> v1::ProviderProfile {
  v1::ProviderProfile {
    id: uuid::Uuid::now_v7().to_string(),
    protocol: v1::RemoteProviderProtocol::OpenaiResponses as i32,
    ..Default::default()
  }
}

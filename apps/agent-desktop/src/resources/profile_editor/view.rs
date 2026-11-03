use super::ProfileEditor;
use gpui_kit::{
  component::{
    Disableable, Sizable,
    button::Button,
    input::{Input, InputState},
  },
  *,
};
impl Render for ProfileEditor {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.reset_inputs {
      self.name = cx.new(|cx| InputState::new(window, cx).placeholder("Profile name"));
      self.endpoint =
        cx.new(|cx| InputState::new(window, cx).placeholder("HTTPS endpoint or loopback HTTP"));
      self.model = cx.new(|cx| InputState::new(window, cx).placeholder("Model identifier"));
      self.reset_inputs = false;
    }
    let blocked = self.pending || self.client.is_none();
    if self.credential.is_none() && !blocked {
      self.credential = Some(cx.new(|cx| {
        InputState::new(window, cx)
          .masked(true)
          .placeholder("New key; blank preserves a saved key")
      }));
    }
    let mut content = div().flex().flex_col().gap_2().mt_4()
      .child("Remote provider profiles")
      .child("Select a preset to create a new profile, or select a saved profile to edit. Model defaults are examples; probe checks the provider catalog.")
      .child(div().flex().flex_wrap().gap_2().children(agent_config::REMOTE_PROVIDER_PRESETS.iter().enumerate().map(|(index, preset)| {
        Button::new(("provider-preset", index)).small().label(preset.name).disabled(blocked)
          .on_click(cx.listener(move |this, _, window, cx| this.preset(index, window, cx)))
      })))
      .child(div().flex().flex_wrap().gap_2().children(self.profiles.iter().enumerate().map(|(index, profile)| {
        let profile = profile.clone();
        Button::new(("provider-profile", index)).small().label(profile.name.clone()).disabled(blocked)
          .on_click(cx.listener(move |this, _, window, cx| this.edit(profile.clone(), window, cx)))
      })))
      .child(Input::new(&self.name).id("provider-profile-name").disabled(blocked))
      .child(Input::new(&self.endpoint).id("provider-profile-endpoint").disabled(blocked))
      .child(Input::new(&self.model).id("provider-profile-model").disabled(blocked));
    if let Some(credential) = &self.credential {
      content = content.child(
        Input::new(credential)
          .id("provider-profile-key")
          .disabled(blocked),
      );
    }
    content
      .child(
        div()
          .flex()
          .gap_2()
          .child(
            Button::new("provider-profile-probe")
              .small()
              .label("Probe models")
              .disabled(blocked)
              .on_click(cx.listener(|this, _, _, cx| this.submit(true, cx))),
          )
          .child(
            Button::new("provider-profile-save")
              .small()
              .label("Save profile")
              .disabled(blocked)
              .on_click(cx.listener(|this, _, _, cx| this.submit(false, cx))),
          )
          .child(
            Button::new("provider-profile-refresh")
              .small()
              .label("Refresh profiles")
              .disabled(blocked)
              .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
          ),
      )
      .child(self.message.clone())
  }
}

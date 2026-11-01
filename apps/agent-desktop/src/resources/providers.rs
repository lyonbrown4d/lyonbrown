use super::state::{Reply, ResourcePane};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, button::Button};
use gpui_kit::*;

impl ResourcePane {
  pub(super) fn upload_environment(
    &mut self,
    provider: String,
    name: String,
    cx: &mut Context<Self>,
  ) {
    if !self.providers.iter().any(|listed| listed.id == provider) {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    self.request(
      true,
      async move {
        let definition = agent_config::provider_definition(&provider)
          .ok_or_else(|| anyhow::anyhow!("Unknown provider"))?;
        anyhow::ensure!(
          definition.credential_env_names.contains(&name.as_str()),
          "Environment name is not allowed for this provider"
        );
        // Read the value only in the Tokio command adapter. Never send it through a GPUI completion.
        let credential = std::env::var(&name)
          .map_err(|_| anyhow::anyhow!("Selected client environment variable is unavailable"))?;
        anyhow::ensure!(
          !credential.trim().is_empty() && !credential.contains('\0'),
          "Selected environment credential is invalid"
        );
        client
          .put_provider_credential(&provider, credential.into_bytes())
          .await
          .map_err(|_| {
            anyhow::anyhow!("Credential upload failed; refresh provider state before trying again")
          })?;
        Ok(Reply::Changed(
          "Credential uploaded to the authenticated daemon".into(),
        ))
      },
      cx,
    );
  }
  pub(super) fn remove_credential(&mut self, provider: String, cx: &mut Context<Self>) {
    if !self.providers.iter().any(|listed| listed.id == provider) {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    self.request(
      true,
      async move {
        client
          .remove_provider_credential(&provider)
          .await
          .map_err(|_| {
            anyhow::anyhow!("Credential removal failed; refresh provider state before trying again")
          })?;
        Ok(Reply::Changed("Uploaded credential removed".into()))
      },
      cx,
    );
  }
  pub(super) fn providers_view(&self, cx: &mut Context<Self>) -> AnyElement {
    div().flex().flex_col().gap_3()
      .child("Providers and credentials")
      .child("Client environment detection is metadata only. Upload explicitly transfers the selected variable to the connected daemon's encrypted credential store.")
      .children(self.providers.iter().enumerate().map(|(index, provider)| {
        let id = provider.id.clone();
        let hint = self.environment.get(&provider.id);
        let upload = hint.filter(|hint| hint.state == agent_config::CredentialState::Detected)
          .and_then(|hint| hint.environment_name.clone());
        let mut row = div().p_3().border_1().border_color(cx.theme().border).rounded_md().flex().flex_col().gap_2()
          .child(format!("{} · {}", provider.name, if provider.runtime_supported { "Runtime supported" } else { "Runtime unavailable" }))
          .child(format!("Daemon credential: {} · {} · {}", provider.credential_state, provider.credential_source, provider.credential_environment))
          .child(format!("Protocols: {}", provider.protocols.join(", ")))
          .child(format!("Configured models: {}", provider.models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>().join(", ")))
          .child(format!("Client environment: {}", hint.map_or_else(|| "missing".into(), |hint| format!("{:?} {}", hint.state, hint.environment_name.as_deref().unwrap_or("")))));
        if let Some(name) = upload {
          let provider = id.clone();
          row = row.child(Button::new(("provider-upload", index)).small().label(format!("Upload {name}"))
            .disabled(self.busy || self.pending_mutations > 0 || !self.ready())
            .on_click(cx.listener(move |this, _, _, cx| this.upload_environment(provider.clone(), name.clone(), cx))));
        }
        row.child(Button::new(("provider-remove", index)).small().label("Remove uploaded credential")
          .disabled(self.busy || self.pending_mutations > 0 || !self.ready())
          .on_click(cx.listener(move |this, _, _, cx| this.remove_credential(id.clone(), cx))))
      }))
      .into_any_element()
  }
}

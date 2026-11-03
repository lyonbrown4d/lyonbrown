use super::ProfileEditor;
use agent_protocol::v1;
use gpui_kit::*;
impl ProfileEditor {
  pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    if self.pending {
      return;
    }
    self.pending = true;
    let generation = self.generation;
    let request = self
      .runtime
      .spawn(async move { client.list_provider_profiles().await });
    cx.spawn(async move |view, cx| {
      let result = request.await;
      let _ = view.update(cx, |view, cx| {
        view.pending = false;
        if view.generation == generation {
          match result {
            Ok(Ok(profiles)) => view.profiles = profiles,
            _ => view.message = "Profile list unavailable. Refresh to try again.".into(),
          }
        }
        cx.notify();
      });
    })
    .detach();
  }
  pub(super) fn submit(&mut self, probe: bool, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    if self.pending {
      return;
    }
    let profile = self.snapshot(cx);
    let credential = self
      .credential
      .take()
      .map(|input| input.read(cx).value().to_string())
      .unwrap_or_default();
    if profile.name.trim().is_empty()
      || profile.model.trim().is_empty()
      || profile.base_url.is_empty()
      || credential.len() > 16384
      || credential.chars().any(char::is_control)
      || (profile.revision == 0 && credential.trim().is_empty())
    {
      self.message =
        "Enter a name, endpoint, model and a single-line key for a new profile.".into();
      cx.notify();
      return;
    }
    self.pending = true;
    self.message = if probe {
      "Requesting provider model catalog…"
    } else {
      "Saving profile…"
    }
    .into();
    let generation = self.generation;
    let request = self.runtime.spawn(async move {
      if probe {
        let result = client
          .probe_provider_profile(v1::ProbeProviderProfileRequest {
            profile: Some(profile),
            credential: credential.into_bytes(),
          })
          .await?;
        Ok::<_, anyhow::Error>((
          None,
          format!("{} · {} models", result.message, result.model_ids.len()),
        ))
      } else {
        let saved = client
          .put_provider_profile(v1::PutProviderProfileRequest {
            profile: Some(profile),
            credential: credential.into_bytes(),
          })
          .await?;
        Ok((
          Some(saved),
          "Profile saved. Refresh Models to select it for a session.".into(),
        ))
      }
    });
    cx.spawn(async move |view, cx| {
      let result = request.await;
      let _ = view.update(cx, |view, cx| {
        view.pending = false;
        if view.generation == generation {
          match result {
            Ok(Ok((saved, message))) => {
              if let Some(profile) = saved {
                view.profiles.retain(|previous| previous.id != profile.id);
                view.profiles.push(profile.clone());
                view.profile = profile;
              }
              view.message = message;
            }
            _ => view.message = "Request failed. Refresh profiles before retrying a save; re-enter the key when needed.".into(),
          }
        }
        cx.notify();
      });
    }).detach();
  }
}

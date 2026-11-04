use crate::Daemon;
use agent_core::provider_profiles::{ProviderProfile, RemoteProviderProtocol};
use agent_protocol::v1;
use agent_secrets::ExposeSecret;
use tonic::Status;

pub(crate) fn storage_error(error: anyhow::Error) -> Status {
  use agent_storage::ProviderProfileError;
  match error.downcast_ref::<ProviderProfileError>() {
    Some(ProviderProfileError::Conflict) => Status::aborted("provider profile revision changed"),
    Some(ProviderProfileError::NotFound) => Status::not_found("provider profile not found"),
    Some(ProviderProfileError::Capacity) => {
      Status::resource_exhausted("provider profile capacity reached")
    }
    Some(ProviderProfileError::Invalid) => Status::invalid_argument("invalid provider profile"),
    None => Status::internal("provider profile storage unavailable"),
  }
}
fn valid_text(value: &str, max: usize) -> bool {
  !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
pub(crate) fn validate(input: v1::ProviderProfile) -> Result<ProviderProfile, Status> {
  if uuid::Uuid::parse_str(&input.id).is_err()
    || !valid_text(&input.name, 100)
    || !valid_text(&input.model, 256)
  {
    return Err(Status::invalid_argument(
      "invalid provider identity, name or model",
    ));
  }
  let protocol = match v1::RemoteProviderProtocol::try_from(input.protocol) {
    Ok(v1::RemoteProviderProtocol::OpenaiResponses) => RemoteProviderProtocol::OpenaiResponses,
    Ok(v1::RemoteProviderProtocol::OpenaiCompletions) => RemoteProviderProtocol::OpenaiCompletions,
    Ok(v1::RemoteProviderProtocol::AnthropicMessages) => RemoteProviderProtocol::AnthropicMessages,
    Ok(v1::RemoteProviderProtocol::GeminiGenerateContent) => {
      RemoteProviderProtocol::GeminiGenerateContent
    }
    _ => return Err(Status::invalid_argument("unsupported remote protocol")),
  };
  let endpoint = &input.base_url;
  let url =
    url::Url::parse(endpoint).map_err(|_| Status::invalid_argument("invalid provider URL"))?;
  let local = match url.host() {
    Some(url::Host::Domain(host)) => host == "localhost",
    Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
    Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
    None => false,
  };
  if endpoint.len() > 2048
    || endpoint.chars().any(char::is_whitespace)
    || !url.username().is_empty()
    || url.password().is_some()
    || url.query().is_some()
    || url.fragment().is_some()
    || !(url.scheme() == "https" || (url.scheme() == "http" && local))
    || url.host().is_none()
  {
    return Err(Status::invalid_argument(
      "provider URL requires HTTPS or loopback HTTP without credentials, query or fragment",
    ));
  }
  Ok(ProviderProfile {
    id: input.id,
    name: input.name,
    protocol,
    base_url: url.to_string(),
    model: input.model,
    revision: input.revision,
    credential_id: String::new(),
  })
}
fn validate_profile_privacy(
  privacy: &mut agent_privacy::PrivacyService,
  profile: &ProviderProfile,
) -> Result<(), Status> {
  for value in [&profile.name, &profile.model, &profile.base_url] {
    let protected = privacy
      .protect_text("provider-profile-validation", value)
      .map_err(|_| Status::failed_precondition("provider privacy protection unavailable"))?;
    if protected != *value {
      return Err(Status::invalid_argument(
        "provider profile contains protected content",
      ));
    }
  }
  Ok(())
}

fn credential(bytes: Vec<u8>) -> Result<Option<agent_secrets::SecretString>, Status> {
  if bytes.is_empty() {
    return Ok(None);
  }
  if bytes.len() > 16 * 1024 {
    return Err(Status::invalid_argument("provider credential too long"));
  }
  let value = String::from_utf8(bytes)
    .map_err(|_| Status::invalid_argument("provider credential must be UTF-8"))?;
  if value.trim().is_empty() || value.chars().any(char::is_control) {
    return Err(Status::invalid_argument(
      "provider credential must be a single line",
    ));
  }
  Ok(Some(agent_secrets::SecretString::from(value)))
}
impl Daemon {
  pub(crate) fn public_profile(
    &self,
    profile: ProviderProfile,
  ) -> Result<v1::ProviderProfile, Status> {
    let configured = self
      .provider_secrets
      .get(&profile.credential_id)
      .map_err(|_| Status::failed_precondition("provider credential store unavailable"))?
      .is_some();
    Ok(v1::ProviderProfile {
      id: profile.id,
      name: profile.name,
      protocol: match profile.protocol {
        RemoteProviderProtocol::OpenaiResponses => {
          v1::RemoteProviderProtocol::OpenaiResponses as i32
        }
        RemoteProviderProtocol::OpenaiCompletions => {
          v1::RemoteProviderProtocol::OpenaiCompletions as i32
        }
        RemoteProviderProtocol::AnthropicMessages => {
          v1::RemoteProviderProtocol::AnthropicMessages as i32
        }
        RemoteProviderProtocol::GeminiGenerateContent => {
          v1::RemoteProviderProtocol::GeminiGenerateContent as i32
        }
      },
      base_url: profile.base_url,
      model: profile.model,
      revision: profile.revision,
      credential_configured: configured,
    })
  }
  pub(crate) async fn save_profile(
    &self,
    input: v1::PutProviderProfileRequest,
  ) -> Result<v1::ProviderProfile, Status> {
    let mut profile = validate(
      input
        .profile
        .ok_or_else(|| Status::invalid_argument("provider profile required"))?,
    )?;
    let secret = credential(input.credential)?;
    let expected = profile.revision;
    if expected == 0 {
      if secret.is_none() {
        return Err(Status::invalid_argument("provider credential required"));
      }
    } else {
      let previous = self
        .store
        .provider_profile(&profile.id)
        .await
        .map_err(storage_error)?
        .ok_or_else(|| Status::not_found("provider profile not found"))?;
      if previous.revision != expected {
        return Err(Status::aborted("provider profile revision changed"));
      }
      profile.credential_id = previous.credential_id;
    }
    profile.revision = expected
      .checked_add(1)
      .ok_or_else(|| Status::invalid_argument("provider revision exhausted"))?;
    {
      let mut privacy = self.privacy.lock().await;
      if let Some(secret) = &secret {
        privacy
          .register_known_secret(secret)
          .map_err(|_| Status::failed_precondition("provider privacy protection unavailable"))?;
      }
      validate_profile_privacy(&mut privacy, &profile)?;
    }
    let new_id = secret
      .as_ref()
      .map(|_| format!("profile-{}", uuid::Uuid::new_v4()));
    if let (Some(secret), Some(id)) = (secret, &new_id) {
      self
        .store_known_credential(id.clone(), secret, false)
        .await?;
      profile.credential_id = id.clone();
    }
    // Protect the final check and publish against concurrent credential uploads.
    let mut privacy = self.privacy.lock().await;
    validate_profile_privacy(&mut privacy, &profile)?;
    let saved = self.store.put_provider_profile(&profile, expected).await;
    drop(privacy);
    match saved {
      Ok(profile) => {
        self.events.notify_waiters();
        self.public_profile(profile)
      }
      Err(error) => {
        // A failed CAS cannot have published this fresh, unique reference.
        if matches!(
          error.downcast_ref::<agent_storage::ProviderProfileError>(),
          Some(
            agent_storage::ProviderProfileError::Conflict
              | agent_storage::ProviderProfileError::Capacity
              | agent_storage::ProviderProfileError::NotFound
              | agent_storage::ProviderProfileError::Invalid
          )
        ) && let Some(id) = new_id
        {
          let _ = self.provider_secrets.remove(&id);
        }
        Err(storage_error(error))
      }
    }
  }
  pub(crate) async fn probe_profile(
    &self,
    input: v1::ProbeProviderProfileRequest,
  ) -> Result<v1::ProviderProbe, Status> {
    let profile = validate(
      input
        .profile
        .ok_or_else(|| Status::invalid_argument("provider profile required"))?,
    )?;
    let secret = match credential(input.credential)? {
      Some(secret) => secret,
      None => {
        let previous = self
          .store
          .provider_profile(&profile.id)
          .await
          .map_err(storage_error)?
          .filter(|previous| previous.revision == profile.revision)
          .ok_or_else(|| Status::aborted("provider profile revision changed"))?;
        self
          .provider_secrets
          .get(&previous.credential_id)
          .map_err(|_| Status::failed_precondition("provider credential store unavailable"))?
          .ok_or_else(|| Status::failed_precondition("provider credential required"))?
      }
    };
    self
      .privacy
      .lock()
      .await
      .register_known_secret(&secret)
      .map_err(|_| Status::failed_precondition("provider privacy protection unavailable"))?;
    Ok(crate::provider_probe::probe(&profile, secret.expose_secret()).await)
  }
}

use agent_core::provider_profiles::{ProviderProfile, RemoteProviderProtocol};
use agent_protocol::v1;
use std::time::Duration;

pub(crate) async fn probe(profile: &ProviderProfile, credential: &str) -> v1::ProviderProbe {
  let result = tokio::time::timeout(Duration::from_secs(5), request(profile, credential)).await;
  match result {
    Ok(Ok(model_ids)) => v1::ProviderProbe {
      reachable: true,
      message: "Connected; model catalog received".into(),
      model_ids,
    },
    Ok(Err(message)) => v1::ProviderProbe {
      reachable: false,
      message,
      model_ids: vec![],
    },
    Err(_) => v1::ProviderProbe {
      reachable: false,
      message: "Provider probe timed out".into(),
      model_ids: vec![],
    },
  }
}
async fn request(profile: &ProviderProfile, credential: &str) -> Result<Vec<String>, String> {
  let client = reqwest::Client::builder()
    .no_proxy()
    .redirect(reqwest::redirect::Policy::none())
    .timeout(Duration::from_secs(5))
    .retry(reqwest::retry::never())
    .build()
    .map_err(|_| "Provider transport unavailable")?;
  let base = profile.base_url.trim_end_matches('/');
  let root = url::Url::parse(base).is_ok_and(|url| matches!(url.path(), "" | "/"));
  let endpoint = if (profile.protocol == RemoteProviderProtocol::AnthropicMessages
    && !base.ends_with("/v1"))
    || root
  {
    format!("{base}/v1/models")
  } else {
    format!("{base}/models")
  };
  let request = client.get(endpoint);
  let mut key = reqwest::header::HeaderValue::from_str(credential)
    .map_err(|_| "Invalid provider credential")?;
  key.set_sensitive(true);
  let request = match profile.protocol {
    RemoteProviderProtocol::AnthropicMessages => request
      .header("x-api-key", key)
      .header("anthropic-version", "2023-06-01"),
    _ => request.bearer_auth(credential),
  };
  let mut response = request
    .send()
    .await
    .map_err(|_| "Provider connection failed")?;
  if !response.status().is_success() {
    return Err(format!(
      "Provider returned HTTP {}",
      response.status().as_u16()
    ));
  }
  let mut bytes = Vec::new();
  while let Some(chunk) = response
    .chunk()
    .await
    .map_err(|_| "Provider response interrupted")?
  {
    if bytes.len().saturating_add(chunk.len()) > 1024 * 1024 {
      return Err("Provider response exceeded limit".into());
    }
    bytes.extend_from_slice(&chunk);
  }
  let body: serde_json::Value =
    serde_json::from_slice(&bytes).map_err(|_| "Provider returned invalid model catalog")?;
  let data = body
    .get("data")
    .and_then(|value| value.as_array())
    .ok_or("Provider returned invalid model catalog")?;
  if data.len() > 100 {
    return Err("Provider model catalog exceeded limit".into());
  }
  data
    .iter()
    .map(|item| {
      item
        .get("id")
        .and_then(|value| value.as_str())
        .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| "Provider returned invalid model catalog".into())
    })
    .collect()
}

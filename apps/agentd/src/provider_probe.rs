use agent_core::provider_profiles::ProviderProfile;
use agent_protocol::v1;

pub(crate) async fn probe(profile: &ProviderProfile, credential: &str) -> v1::ProviderProbe {
  match model_provider::list_models(
    profile.runtime_provider(),
    credential,
    Some(&profile.base_url),
  )
  .await
  {
    Ok(model_ids) => v1::ProviderProbe {
      reachable: true,
      message: "Connected; model catalog received".into(),
      model_ids,
    },
    Err(_) => v1::ProviderProbe {
      reachable: false,
      message: "Provider catalog unavailable; check endpoint and credential".into(),
      model_ids: vec![],
    },
  }
}

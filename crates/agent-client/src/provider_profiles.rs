use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;
impl AgentClient {
  pub async fn list_provider_profiles(&self) -> Result<Vec<v1::ProviderProfile>> {
    Ok(
      self
        .inner
        .clone()
        .list_provider_profiles(v1::Empty {})
        .await?
        .into_inner()
        .profiles,
    )
  }
  pub async fn put_provider_profile(
    &self,
    request: v1::PutProviderProfileRequest,
  ) -> Result<v1::ProviderProfile> {
    Ok(
      self
        .inner
        .clone()
        .put_provider_profile(request)
        .await?
        .into_inner(),
    )
  }
  pub async fn remove_provider_profile(&self, id: &str, expected_revision: u64) -> Result<()> {
    self
      .inner
      .clone()
      .remove_provider_profile(v1::RemoveProviderProfileRequest {
        id: id.into(),
        expected_revision,
      })
      .await?;
    Ok(())
  }
  pub async fn probe_provider_profile(
    &self,
    request: v1::ProbeProviderProfileRequest,
  ) -> Result<v1::ProviderProbe> {
    Ok(
      self
        .inner
        .clone()
        .probe_provider_profile(request)
        .await?
        .into_inner(),
    )
  }
}

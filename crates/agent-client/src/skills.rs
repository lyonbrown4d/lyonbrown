use agent_protocol::v1;
use anyhow::Result;

use crate::AgentClient;

impl AgentClient {
  pub async fn list_skills(&self, scope: &str, session_id: &str) -> Result<Vec<v1::Skill>> {
    Ok(
      self
        .skills
        .clone()
        .list_skills(v1::ListSkillsRequest {
          scope: scope.into(),
          session_id: session_id.into(),
        })
        .await?
        .into_inner()
        .skills,
    )
  }

  pub async fn inspect_skill(
    &self,
    source: &str,
    scope: &str,
    session_id: &str,
  ) -> Result<v1::SkillInspection> {
    Ok(
      self
        .skills
        .clone()
        .inspect_skill(v1::InspectSkillRequest {
          source: source.into(),
          scope: scope.into(),
          session_id: session_id.into(),
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn install_skill(
    &self,
    source: &str,
    scope: &str,
    session_id: &str,
    expected_sha256: &str,
  ) -> Result<v1::Skill> {
    Ok(
      self
        .skills
        .clone()
        .install_skill(v1::InstallSkillRequest {
          source: source.into(),
          scope: scope.into(),
          session_id: session_id.into(),
          expected_sha256: expected_sha256.into(),
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn remove_skill(
    &self,
    name: &str,
    scope: &str,
    session_id: &str,
    expected_sha256: &str,
  ) -> Result<v1::Skill> {
    Ok(
      self
        .skills
        .clone()
        .remove_skill(v1::RemoveSkillRequest {
          name: name.into(),
          scope: scope.into(),
          session_id: session_id.into(),
          expected_sha256: expected_sha256.into(),
        })
        .await?
        .into_inner(),
    )
  }
}

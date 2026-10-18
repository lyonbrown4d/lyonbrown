use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;
impl AgentClient {
  pub async fn watch_files(
    &self,
    session_id: &str,
  ) -> Result<tonic::Streaming<v1::WorkspaceChange>> {
    Ok(
      self
        .workspace
        .clone()
        .watch_files(v1::SessionRequest {
          session_id: session_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn list_files(
    &self,
    session_id: &str,
    directory: &str,
  ) -> Result<Vec<v1::WorkspaceEntry>> {
    Ok(
      self
        .workspace
        .clone()
        .list_files(v1::ListFilesRequest {
          session_id: session_id.into(),
          directory: directory.into(),
        })
        .await?
        .into_inner()
        .entries,
    )
  }
  pub async fn read_file(&self, session_id: &str, path: &str) -> Result<v1::WorkspaceFile> {
    Ok(
      self
        .workspace
        .clone()
        .read_file(v1::ReadFileRequest {
          session_id: session_id.into(),
          path: path.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn write_file(
    &self,
    session_id: &str,
    path: &str,
    content: &str,
    expected_revision: &str,
  ) -> Result<v1::WorkspaceFile> {
    Ok(
      self
        .workspace
        .clone()
        .write_file(v1::WriteFileRequest {
          session_id: session_id.into(),
          path: path.into(),
          content: content.into(),
          expected_revision: expected_revision.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn workspace_diff(&self, session_id: &str, path: &str) -> Result<String> {
    Ok(
      self
        .workspace
        .clone()
        .get_diff(v1::DiffRequest {
          session_id: session_id.into(),
          path: path.into(),
        })
        .await?
        .into_inner()
        .patch,
    )
  }
}

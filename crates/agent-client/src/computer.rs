use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;

impl AgentClient {
  /// Attach a trusted interactive-user helper. This channel alone grants no
  /// native-input approval; every action remains daemon-owned and fenced.
  pub async fn attach_computer_runtime(
    &self,
    responses: impl futures::Stream<Item = v1::ComputerRuntimeResponse> + Send + 'static,
  ) -> Result<tonic::Streaming<v1::ComputerRuntimeRequest>> {
    Ok(
      self
        .computer_runtime
        .clone()
        .attach(responses)
        .await?
        .into_inner(),
    )
  }
}

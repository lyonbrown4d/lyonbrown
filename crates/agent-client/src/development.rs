use crate::AgentClient;
use agent_protocol::v1;
use anyhow::{Context, Result};

impl AgentClient {
  /// Read node identity and the optional development incarnation from one authenticated RPC.
  pub async fn development_node(&self) -> Result<(v1::Node, Option<String>)> {
    let response = self.inner.clone().get_node(v1::Empty {}).await?;
    let incarnation = response
      .metadata()
      .get("x-lyonbrown-dev-instance")
      .map(|value| {
        value
          .to_str()
          .map(str::to_owned)
          .context("invalid development instance response metadata")
      })
      .transpose()?;
    Ok((response.into_inner(), incarnation))
  }
}

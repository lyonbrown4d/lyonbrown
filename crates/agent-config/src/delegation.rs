use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Deserialize;

/// Operator-selected routes; a durable active pairing is still required at dispatch.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationTargetSettings {
  pub node_id: String,
  pub endpoint: String,
  pub capabilities: BTreeSet<String>,
}

pub fn validate_delegation_targets(targets: &[DelegationTargetSettings]) -> Result<()> {
  ensure!(
    targets.len() <= 32,
    "at most 32 delegation targets are supported"
  );
  let mut seen = BTreeSet::new();
  for target in targets {
    ensure!(
      !target.node_id.is_empty()
        && target.node_id.len() <= 256
        && target
          .node_id
          .bytes()
          .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.')),
      "invalid delegation node identifier"
    );
    ensure!(
      seen.insert(&target.node_id),
      "duplicate delegation node identifier"
    );
    let endpoint = url::Url::parse(&target.endpoint)
      .map_err(|_| anyhow::anyhow!("invalid delegation target origin"))?;
    ensure!(
      target.endpoint.len() <= 2048
        && endpoint.scheme() == "https"
        && endpoint.host_str().is_some()
        && endpoint.username().is_empty()
        && endpoint.password().is_none()
        && endpoint.query().is_none()
        && endpoint.fragment().is_none()
        && endpoint.path() == "/",
      "delegation target must be a credential-free HTTPS origin"
    );
    ensure!(
      target.capabilities.iter().all(|capability| matches!(
        capability.as_str(),
        "workspace.read" | "workspace.write.approval"
      )),
      "unsupported delegation capability"
    );
  }
  Ok(())
}

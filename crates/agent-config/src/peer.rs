use std::net::SocketAddr;

use anyhow::{Result, ensure};
use serde::Deserialize;
use url::Url;

/// Explicit opt-in for the mutually authenticated peer endpoint.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerSettings {
  pub listen: SocketAddr,
  pub advertise_url: String,
  #[serde(default)]
  pub mdns: bool,
}

impl PeerSettings {
  pub fn validate(&self) -> Result<()> {
    ensure!(self.listen.port() != 0, "peer listen port must be nonzero");
    let url = Url::parse(&self.advertise_url)
      .map_err(|_| anyhow::anyhow!("peer advertise_url must be an absolute HTTPS URL"))?;
    ensure!(
      url.scheme() == "https"
        && url.host_str().is_some_and(|host| !host.contains('*'))
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && matches!(url.path(), "" | "/")
        && url.port_or_known_default() == Some(self.listen.port()),
      "peer advertise_url must be a credential-free HTTPS origin using the listen port"
    );
    if let Some(url::Host::Ipv4(ip)) = url.host() {
      ensure!(
        !ip.is_unspecified(),
        "peer advertise address must be reachable"
      );
    }
    if let Some(url::Host::Ipv6(ip)) = url.host() {
      ensure!(
        !ip.is_unspecified(),
        "peer advertise address must be reachable"
      );
    }
    Ok(())
  }
}

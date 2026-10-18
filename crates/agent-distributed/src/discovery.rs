use std::{collections::HashMap, net::SocketAddr};

use anyhow::{Context, Result, ensure};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

pub const SERVICE_TYPE: &str = "_lyonbrown-agent._tcp.local.";

/// Discovery data is untrusted. It carries neither pairing authority nor credentials.
#[derive(Clone, Debug)]
pub struct Candidate {
  pub node_id: String,
  pub protocol_version: u32,
  pub endpoints: Vec<String>,
  pub service_name: String,
}

#[derive(Clone, Debug)]
pub enum DiscoveryEvent {
  Candidate(Candidate),
  Removed(String),
}

pub struct Discovery {
  daemon: ServiceDaemon,
  receiver: mdns_sd::Receiver<ServiceEvent>,
  fullname: String,
  local_node: String,
}

impl Discovery {
  pub fn advertise(node_id: &str, protocol_version: u32, address: SocketAddr) -> Result<Self> {
    ensure!(
      !node_id.is_empty() && node_id.len() <= 63 && address.port() != 0,
      "invalid mDNS advertisement"
    );
    let daemon = ServiceDaemon::new().context("mDNS discovery is unavailable on this network")?;
    let properties = HashMap::from([
      ("node_id".into(), node_id.to_owned()),
      ("protocol".into(), protocol_version.to_string()),
    ]);
    let host = format!("{node_id}.local.");
    let service = if address.ip().is_unspecified() {
      ServiceInfo::new(SERVICE_TYPE, node_id, &host, "", address.port(), properties)?
        .enable_addr_auto()
    } else {
      ServiceInfo::new(
        SERVICE_TYPE,
        node_id,
        &host,
        address.ip(),
        address.port(),
        properties,
      )?
    };
    let fullname = service.get_fullname().to_owned();
    daemon.register(service)?;
    let receiver = daemon.browse(SERVICE_TYPE)?;
    Ok(Self {
      daemon,
      receiver,
      fullname,
      local_node: node_id.into(),
    })
  }

  pub async fn next(&self) -> Result<DiscoveryEvent> {
    loop {
      match self
        .receiver
        .recv_async()
        .await
        .context("mDNS discovery stopped")?
      {
        ServiceEvent::ServiceResolved(service) => {
          let Some(node) = service.get_property_val_str("node_id") else {
            continue;
          };
          if node == self.local_node || node.is_empty() || node.len() > 128 {
            continue;
          }
          let Some(protocol) = service
            .get_property_val_str("protocol")
            .and_then(|value| value.parse::<u32>().ok())
          else {
            continue;
          };
          let endpoints = service
            .get_addresses()
            .iter()
            .map(|address| address.to_ip_addr())
            .filter(
              |address| !matches!(address, std::net::IpAddr::V6(ip) if ip.is_unicast_link_local()),
            )
            .map(|address| format!("https://{}", SocketAddr::new(address, service.get_port())))
            .collect::<Vec<_>>();
          if endpoints.is_empty() {
            continue;
          }
          return Ok(DiscoveryEvent::Candidate(Candidate {
            node_id: node.into(),
            protocol_version: protocol,
            endpoints,
            service_name: service.fullname,
          }));
        }
        ServiceEvent::ServiceRemoved(_, fullname) => return Ok(DiscoveryEvent::Removed(fullname)),
        _ => {}
      }
    }
  }
}

impl Drop for Discovery {
  fn drop(&mut self) {
    let _ = self.daemon.unregister(&self.fullname);
    let _ = self.daemon.stop_browse(SERVICE_TYPE);
    let _ = self.daemon.shutdown();
  }
}

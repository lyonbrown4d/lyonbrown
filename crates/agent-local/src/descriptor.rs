use crate::{DiscoveryError, SecretToken};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;
use uuid::Uuid;

pub const MAX_DISCOVERY_BYTES: usize = 4 * 1024;
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
  GrpcTcp,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ScopeFingerprint(String);

impl ScopeFingerprint {
  pub fn parse(value: &str) -> Result<Self, DiscoveryError> {
    if value.len() == 64
      && value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
      Ok(Self(value.to_owned()))
    } else {
      Err(DiscoveryError::InvalidDescriptor(
        "scope fingerprint must be 64 lowercase hexadecimal characters".to_owned(),
      ))
    }
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl fmt::Debug for ScopeFingerprint {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_tuple("ScopeFingerprint")
      .field(&self.0)
      .finish()
  }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DiscoveryDescriptor {
  address: SocketAddr,
  pid: u32,
  server_instance_id: Uuid,
  token: SecretToken,
  scope_fingerprint: ScopeFingerprint,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DescriptorWire {
  schema_version: u32,
  transport: Transport,
  address: String,
  pid: u32,
  server_instance_id: String,
  token: String,
  scope_fingerprint: String,
}

impl DiscoveryDescriptor {
  pub fn new(
    address: SocketAddr,
    pid: u32,
    server_instance_id: Uuid,
    token: SecretToken,
    scope_fingerprint: ScopeFingerprint,
  ) -> Result<Self, DiscoveryError> {
    validate_address(address)?;
    if pid == 0 {
      return Err(DiscoveryError::ZeroPid);
    }
    if server_instance_id.is_nil() {
      return Err(DiscoveryError::InvalidDescriptor(
        "server instance id must not be nil".to_owned(),
      ));
    }
    Ok(Self {
      address,
      pid,
      server_instance_id,
      token,
      scope_fingerprint,
    })
  }

  pub fn from_json(encoded: &[u8]) -> Result<Self, DiscoveryError> {
    if encoded.len() > MAX_DISCOVERY_BYTES {
      return Err(DiscoveryError::DescriptorTooLarge {
        actual: encoded.len(),
        limit: MAX_DISCOVERY_BYTES,
      });
    }
    let wire: DescriptorWire = serde_json::from_slice(encoded)
      .map_err(|error| DiscoveryError::InvalidDescriptor(error.to_string()))?;
    if wire.schema_version != SCHEMA_VERSION {
      return Err(DiscoveryError::InvalidDescriptor(
        "unsupported discovery schema version".to_owned(),
      ));
    }
    if wire.transport != Transport::GrpcTcp {
      return Err(DiscoveryError::InvalidDescriptor(
        "unsupported local transport".to_owned(),
      ));
    }
    let address = wire.address.parse::<SocketAddr>().map_err(|_| {
      DiscoveryError::InvalidDescriptor("address must be numeric IP:port".to_owned())
    })?;
    let server_instance_id = Uuid::parse_str(&wire.server_instance_id)
      .map_err(|_| DiscoveryError::InvalidDescriptor("invalid server instance id".to_owned()))?;
    Self::new(
      address,
      wire.pid,
      server_instance_id,
      SecretToken::parse(&wire.token)?,
      ScopeFingerprint::parse(&wire.scope_fingerprint)?,
    )
  }

  pub fn to_json(&self) -> Result<Vec<u8>, DiscoveryError> {
    let wire = DescriptorWire {
      schema_version: SCHEMA_VERSION,
      transport: Transport::GrpcTcp,
      address: self.address.to_string(),
      pid: self.pid,
      server_instance_id: self.server_instance_id.to_string(),
      token: self.token.expose_for_transport(),
      scope_fingerprint: self.scope_fingerprint.0.clone(),
    };
    let encoded = serde_json::to_vec(&wire)
      .map_err(|error| DiscoveryError::InvalidDescriptor(error.to_string()))?;
    if encoded.len() > MAX_DISCOVERY_BYTES {
      return Err(DiscoveryError::DescriptorTooLarge {
        actual: encoded.len(),
        limit: MAX_DISCOVERY_BYTES,
      });
    }
    Ok(encoded)
  }

  pub fn schema_version(&self) -> u32 {
    SCHEMA_VERSION
  }

  pub fn transport(&self) -> Transport {
    Transport::GrpcTcp
  }

  pub fn address(&self) -> SocketAddr {
    self.address
  }

  pub fn pid(&self) -> u32 {
    self.pid
  }

  pub fn server_instance_id(&self) -> Uuid {
    self.server_instance_id
  }

  pub fn token(&self) -> &SecretToken {
    &self.token
  }

  pub fn scope_fingerprint(&self) -> &ScopeFingerprint {
    &self.scope_fingerprint
  }
}

impl fmt::Debug for DiscoveryDescriptor {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("DiscoveryDescriptor")
      .field("schema_version", &SCHEMA_VERSION)
      .field("transport", &Transport::GrpcTcp)
      .field("address", &self.address)
      .field("pid", &self.pid)
      .field("server_instance_id", &self.server_instance_id)
      .field("token", &self.token)
      .field("scope_fingerprint", &self.scope_fingerprint)
      .finish()
  }
}

fn validate_address(address: SocketAddr) -> Result<(), DiscoveryError> {
  if !address.ip().is_loopback() {
    return Err(DiscoveryError::NonLoopbackAddress(address));
  }
  if address.port() == 0 {
    return Err(DiscoveryError::ZeroPort);
  }
  Ok(())
}

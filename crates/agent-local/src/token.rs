use crate::DiscoveryError;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use secrecy::{ExposeSecret, SecretBox};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use subtle::ConstantTimeEq;

const TOKEN_BYTES: usize = 32;

pub struct SecretToken(SecretBox<[u8; TOKEN_BYTES]>);

impl SecretToken {
  pub fn generate() -> Result<Self, DiscoveryError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| DiscoveryError::TokenGeneration)?;
    Ok(Self(SecretBox::new(Box::new(bytes))))
  }

  pub fn parse(encoded: &str) -> Result<Self, DiscoveryError> {
    if encoded.len() != 43 || encoded.contains('=') {
      return Err(DiscoveryError::InvalidToken);
    }
    let decoded = URL_SAFE_NO_PAD
      .decode(encoded)
      .map_err(|_| DiscoveryError::InvalidToken)?;
    let bytes: [u8; TOKEN_BYTES] = decoded
      .try_into()
      .map_err(|_| DiscoveryError::InvalidToken)?;
    if URL_SAFE_NO_PAD.encode(bytes) != encoded {
      return Err(DiscoveryError::InvalidToken);
    }
    Ok(Self(SecretBox::new(Box::new(bytes))))
  }

  pub fn expose_for_transport(&self) -> String {
    URL_SAFE_NO_PAD.encode(self.0.expose_secret())
  }

  pub(crate) fn constant_time_eq(&self, candidate: &Self) -> bool {
    bool::from(self.0.expose_secret().ct_eq(candidate.0.expose_secret()))
  }
}

impl Clone for SecretToken {
  fn clone(&self) -> Self {
    Self(SecretBox::new(Box::new(*self.0.expose_secret())))
  }
}

impl PartialEq for SecretToken {
  fn eq(&self, other: &Self) -> bool {
    self.constant_time_eq(other)
  }
}

impl Eq for SecretToken {}

impl fmt::Debug for SecretToken {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("SecretToken([REDACTED])")
  }
}

impl fmt::Display for SecretToken {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("[REDACTED]")
  }
}

impl Serialize for SecretToken {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(&self.expose_for_transport())
  }
}

impl<'de> Deserialize<'de> for SecretToken {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let encoded = String::deserialize(deserializer)?;
    Self::parse(&encoded).map_err(serde::de::Error::custom)
  }
}

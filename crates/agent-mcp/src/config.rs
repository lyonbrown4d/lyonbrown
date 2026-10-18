use std::{
  collections::{BTreeMap, BTreeSet},
  path::PathBuf,
  time::Duration,
};

use agent_runtime::{Result, RuntimeError};
use http::{HeaderName, HeaderValue};
use url::Url;

/// Deliberately neither Debug nor Serialize: headers can contain credentials.
#[derive(Clone, Default)]
pub struct SecretHeaders(pub BTreeMap<String, String>);

/// Supplied only by trusted daemon configuration, never model arguments.
#[derive(Clone)]
pub struct StdioConfig {
  pub executable: PathBuf,
  pub arguments: Vec<String>,
  /// Explicit complete environment, including any necessary OS variables.
  /// The daemon must select these from its platform allowlist.
  pub environment: BTreeMap<String, String>,
  pub stderr_directory: PathBuf,
}

#[derive(Clone)]
pub enum McpTransport {
  Stdio(StdioConfig),
  StreamableHttp { url: String, headers: SecretHeaders },
}

#[derive(Clone)]
pub struct McpServerConfig {
  pub id: String,
  pub transport: McpTransport,
  pub allowed_tools: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct McpLimits {
  pub connection_timeout: Duration,
  pub call_timeout: Duration,
  pub max_tools: usize,
  pub max_text_bytes: usize,
  pub max_image_bytes: usize,
}

impl Default for McpLimits {
  fn default() -> Self {
    Self {
      connection_timeout: Duration::from_secs(15),
      call_timeout: Duration::from_secs(60),
      max_tools: 64,
      max_text_bytes: 1024 * 1024,
      max_image_bytes: 16 * 1024 * 1024,
    }
  }
}

pub(crate) fn invalid(message: impl Into<String>) -> RuntimeError {
  RuntimeError::Configuration(message.into())
}

pub(crate) fn valid_id(value: &str) -> bool {
  !value.is_empty()
    && value.len() <= 128
    && value
      .bytes()
      .all(|v| v.is_ascii_alphanumeric() || b"_-.".contains(&v))
}

impl McpLimits {
  pub(crate) fn validate(&self) -> Result<()> {
    if self.connection_timeout.is_zero()
      || self.call_timeout.is_zero()
      || self.connection_timeout > Duration::from_secs(300)
      || self.call_timeout > Duration::from_secs(3600)
      || !(1..=64).contains(&self.max_tools)
      || !(1..=1024 * 1024).contains(&self.max_text_bytes)
      || !(1..=16 * 1024 * 1024).contains(&self.max_image_bytes)
    {
      return Err(invalid("MCP limits exceed permitted bounds"));
    }
    Ok(())
  }
}

impl McpServerConfig {
  pub(crate) fn validate(&self) -> Result<()> {
    if !valid_id(&self.id)
      || self.allowed_tools.is_empty()
      || self.allowed_tools.len() > 64
      || self.allowed_tools.iter().any(|v| !valid_id(v))
    {
      return Err(invalid("MCP server/tool allowlist is invalid"));
    }
    match &self.transport {
      McpTransport::Stdio(config) => {
        if !config.executable.is_absolute() || !config.stderr_directory.is_absolute() {
          return Err(invalid(
            "MCP executable and stderr directory must be absolute",
          ));
        }
        if config
          .environment
          .keys()
          .any(|key| key.is_empty() || key.contains(['=', '\0']))
          || config
            .environment
            .values()
            .any(|value| value.contains('\0'))
          || config.arguments.iter().any(|value| value.contains('\0'))
        {
          return Err(invalid("MCP process configuration contains invalid values"));
        }
      }
      McpTransport::StreamableHttp { url, headers } => {
        validate_url(url)?;
        headers.validated()?;
      }
    }
    Ok(())
  }
}

pub(crate) fn validate_url(value: &str) -> Result<Url> {
  let url = Url::parse(value).map_err(|_| invalid("MCP URL is invalid"))?;
  let loopback = url.host_str().is_some_and(|host| {
    host == "localhost"
      || host
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
      || host == "[::1]"
  });
  if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
    || url.host_str().is_none()
    || !url.username().is_empty()
    || url.password().is_some()
    || url.query().is_some()
    || url.fragment().is_some()
  {
    return Err(invalid(
      "MCP URL requires HTTPS or loopback HTTP without credentials, query or fragment",
    ));
  }
  Ok(url)
}

impl SecretHeaders {
  pub(crate) fn validated(&self) -> Result<std::collections::HashMap<HeaderName, HeaderValue>> {
    self
      .0
      .iter()
      .map(|(key, value)| {
        let name = HeaderName::try_from(key).map_err(|_| invalid("invalid MCP header name"))?;
        if [
          "host",
          "accept",
          "content-type",
          "content-length",
          "connection",
          "mcp-session-id",
          "mcp-protocol-version",
        ]
        .contains(&name.as_str())
        {
          return Err(invalid("MCP protocol headers cannot be overridden"));
        }
        let mut value =
          HeaderValue::try_from(value).map_err(|_| invalid("invalid MCP header value"))?;
        value.set_sensitive(true);
        Ok((name, value))
      })
      .collect()
  }
}

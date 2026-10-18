use std::{
  collections::{BTreeMap, BTreeSet},
  path::PathBuf,
};

use anyhow::{Result, ensure};
use serde::Deserialize;
use url::Url;

/// Trusted operator configuration. Values name environment variables rather
/// than embedding credentials in a serializable configuration object.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpSettings {
  pub id: String,
  pub allowed_tools: BTreeSet<String>,
  pub transport: McpTransportSettings,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpTransportSettings {
  Stdio {
    executable: PathBuf,
    #[serde(default)]
    arguments: Vec<String>,
    /// Child environment key -> dotenv/environment variable name.
    #[serde(default)]
    environment: BTreeMap<String, String>,
  },
  StreamableHttp {
    url: String,
    /// HTTP header name -> dotenv/environment variable name.
    #[serde(default)]
    headers: BTreeMap<String, String>,
  },
}

impl std::fmt::Debug for McpTransportSettings {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    // Arguments and invalid URLs can contain secrets; do not format them.
    f.write_str(match self {
      Self::Stdio { .. } => "Stdio([REDACTED])",
      Self::StreamableHttp { .. } => "StreamableHttp([REDACTED])",
    })
  }
}

impl McpSettings {
  pub fn environment_references(&self) -> impl Iterator<Item = &String> {
    match &self.transport {
      McpTransportSettings::Stdio { environment, .. } => environment.values(),
      McpTransportSettings::StreamableHttp { headers, .. } => headers.values(),
    }
  }
}

pub fn validate_mcp(servers: &[McpSettings]) -> Result<()> {
  ensure!(servers.len() <= 64, "at most 64 MCP servers are permitted");
  let mut ids = BTreeSet::new();
  let mut count = 0;
  for server in servers {
    ensure!(
      valid_id(&server.id) && ids.insert(&server.id),
      "invalid or duplicate MCP server id"
    );
    count += server.allowed_tools.len();
    ensure!(
      !server.allowed_tools.is_empty()
        && count <= 64
        && server.allowed_tools.iter().all(|tool| valid_id(tool)),
      "MCP tools require an explicit allowlist of at most 64 tools in total"
    );
    ensure!(
      server
        .environment_references()
        .all(|name| valid_environment_name(name)),
      "invalid MCP environment reference"
    );
    match &server.transport {
      McpTransportSettings::Stdio {
        executable,
        arguments,
        environment,
      } => {
        ensure!(executable.is_absolute(), "MCP executable must be absolute");
        ensure!(
          arguments.len() <= 128
            && arguments
              .iter()
              .all(|arg| arg.len() <= 8192 && !arg.contains('\0')),
          "invalid MCP process arguments"
        );
        ensure!(
          environment.len() <= 64 && environment.keys().all(|name| valid_environment_name(name)),
          "invalid MCP child environment key"
        );
      }
      McpTransportSettings::StreamableHttp { url, headers } => {
        let url = Url::parse(url).map_err(|_| anyhow::anyhow!("invalid MCP URL"))?;
        let local = url.host_str().is_some_and(|host| {
          host == "localhost"
            || host == "[::1]"
            || host
              .parse::<std::net::IpAddr>()
              .is_ok_and(|ip| ip.is_loopback())
        });
        ensure!(
          (url.scheme() == "https" || url.scheme() == "http" && local)
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
          "MCP URL requires HTTPS or loopback HTTP without credentials, query or fragment"
        );
        ensure!(
          headers.len() <= 32
            && headers.keys().all(|name| !name.is_empty()
              && name.len() <= 128
              && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')),
          "invalid MCP header names"
        );
      }
    }
  }
  Ok(())
}

fn valid_id(value: &str) -> bool {
  !value.is_empty()
    && value.len() <= 128
    && value
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
}

fn valid_environment_name(value: &str) -> bool {
  let mut bytes = value.bytes();
  value.len() <= 128
    && bytes
      .next()
      .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
    && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

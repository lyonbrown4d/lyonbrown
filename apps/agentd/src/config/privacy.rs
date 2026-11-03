//! Collect resolved host credentials for the model egress boundary.
use agent_secrets::SecretString;

use super::Config;

impl Config {
  pub(crate) fn privacy_credentials(&self) -> Vec<SecretString> {
    let mut known: Vec<_> = self.gateway_credentials.values().cloned().collect();
    known.extend(self.catalog_credentials.values().cloned().map(Into::into));
    known.extend(self.provider_api_key.iter().cloned().map(Into::into));
    known.extend(self.decision_api_key.iter().cloned().map(Into::into));
    known.extend(self.hub_token.iter().cloned().map(Into::into));
    known.extend(self.memory_embedding_api_key.iter().cloned());
    known.extend(self.memory_vector_api_key.iter().cloned());
    for (name, value) in &self.command_environment {
      if sensitive_name(&name.to_string_lossy())
        && let Some(value) = value.to_str()
      {
        add(&mut known, value);
      }
    }
    for server in &self.mcp {
      match &server.transport {
        agent_mcp::McpTransport::Stdio(process) => {
          for (name, value) in &process.environment {
            if sensitive_name(name) {
              add(&mut known, value);
            }
          }
        }
        agent_mcp::McpTransport::StreamableHttp { headers, .. } => {
          for (name, value) in &headers.0 {
            if sensitive_name(name) {
              add(&mut known, value);
              if let Some(token) = value.strip_prefix("Bearer ") {
                add(&mut known, token);
              }
              if let Some(token) = value.strip_prefix("Basic ") {
                add(&mut known, token);
              }
            }
          }
        }
      }
    }
    known
  }
}

fn sensitive_name(name: &str) -> bool {
  let name = name.to_ascii_uppercase();
  [
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "CREDENTIAL",
    "AUTHORIZATION",
    "COOKIE",
  ]
  .iter()
  .any(|part| name.contains(part))
}

fn add(known: &mut Vec<SecretString>, value: &str) {
  if !value.is_empty() {
    known.push(value.to_owned().into());
  }
}

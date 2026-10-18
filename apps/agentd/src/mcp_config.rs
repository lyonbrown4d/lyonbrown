use agent_config::{McpTransportSettings, Settings};
use agent_mcp::{McpServerConfig, McpTransport, SecretHeaders, StdioConfig};
use std::collections::BTreeMap;

pub(crate) fn resolve(settings: &Settings) -> anyhow::Result<Vec<McpServerConfig>> {
  settings
    .mcp
    .iter()
    .map(|server| {
      let resolve =
        |references: &BTreeMap<String, String>| -> anyhow::Result<BTreeMap<String, String>> {
          references
            .iter()
            .map(|(key, name)| {
              let value = settings
                .mcp_environment_value(name)
                .ok_or_else(|| anyhow::anyhow!("MCP environment reference {name} is missing"))?;
              Ok((key.clone(), value.to_owned()))
            })
            .collect()
        };
      let transport = match &server.transport {
        McpTransportSettings::Stdio {
          executable,
          arguments,
          environment,
        } => {
          // No ambient provider keys, proxy settings, shell hooks or dynamic
          // library injection. Additional variables require explicit references.
          let mut selected: BTreeMap<String, String> = [
            "SystemRoot",
            "WINDIR",
            "TEMP",
            "TMP",
            "TMPDIR",
            "LANG",
            "LC_ALL",
          ]
          .into_iter()
          .filter_map(|key| std::env::var(key).ok().map(|value| (key.into(), value)))
          .collect();
          selected.extend(resolve(environment)?);
          McpTransport::Stdio(StdioConfig {
            executable: executable.clone(),
            arguments: arguments.clone(),
            environment: selected,
            stderr_directory: settings.log_directory.join("mcp"),
          })
        }
        McpTransportSettings::StreamableHttp { url, headers } => McpTransport::StreamableHttp {
          url: url.clone(),
          headers: SecretHeaders(resolve(headers)?),
        },
      };
      Ok(McpServerConfig {
        id: server.id.clone(),
        transport,
        allowed_tools: server.allowed_tools.clone(),
      })
    })
    .collect()
}

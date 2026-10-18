use std::collections::BTreeMap;

use agent_runtime::{Result, ToolDefinition};
use jsonschema::JSONSchema;
use rmcp::model::Tool;
use serde_json::{Value, json};

use crate::{
  McpLimits, McpServerConfig,
  config::{invalid, valid_id},
  connection::Connection,
};

pub(crate) struct CatalogEntry {
  pub schema: Value,
  pub description: Option<String>,
  pub validator: JSONSchema,
}

pub(crate) async fn catalog(
  connection: &Connection,
  config: &McpServerConfig,
  limits: &McpLimits,
) -> Result<BTreeMap<String, CatalogEntry>> {
  let info = connection
    .peer_info()
    .ok_or_else(|| invalid("MCP handshake did not return server information"))?;
  if info.capabilities.tools.is_none() {
    return Err(invalid("MCP server does not advertise tools"));
  }
  let mut cursor = None;
  let mut selected = BTreeMap::new();
  let mut seen = std::collections::BTreeSet::new();
  let mut count = 0;
  loop {
    let request = cursor
      .take()
      .map(|cursor| rmcp::model::PaginatedRequestParams::default().with_cursor(Some(cursor)));
    let page = connection
      .list_tools(request)
      .await
      .map_err(|_| invalid("MCP tool catalog request failed"))?;
    count += page.tools.len();
    if count > 1024 {
      return Err(invalid("MCP server catalog exceeds 1024 tools"));
    }
    for tool in page.tools {
      if !seen.insert(tool.name.to_string()) {
        return Err(invalid("MCP tool catalog contains duplicate names"));
      }
      if config.allowed_tools.contains(tool.name.as_ref()) {
        if selected.len() >= limits.max_tools {
          return Err(invalid("MCP selected catalog exceeds limit"));
        }
        let name = tool.name.to_string();
        selected.insert(name, compile(tool, limits)?);
      }
    }
    match page.next_cursor {
      None => break,
      Some(next) if !next.is_empty() && seen.insert(format!("cursor:{next}")) => {
        cursor = Some(next)
      }
      _ => return Err(invalid("MCP tool catalog cursor repeated")),
    }
  }
  if selected.len() != config.allowed_tools.len() {
    return Err(invalid(
      "configured MCP tool is missing from server catalog",
    ));
  }
  Ok(selected)
}

fn compile(tool: Tool, limits: &McpLimits) -> Result<CatalogEntry> {
  if !valid_id(&tool.name) {
    return Err(invalid("MCP tool has an invalid name"));
  }
  let schema = Value::Object((*tool.input_schema).clone());
  if serde_json::to_vec(&schema)
    .map_err(|_| invalid("cannot encode MCP schema"))?
    .len()
    > limits.max_text_bytes.min(64 * 1024)
    || schema.get("type").and_then(Value::as_str) != Some("object")
    || has_external_reference(&schema)
  {
    return Err(invalid(
      "MCP schema must be a bounded object schema without external references",
    ));
  }
  let validator = JSONSchema::options()
    .with_draft(jsonschema::Draft::Draft202012)
    .compile(&schema)
    .map_err(|_| invalid("MCP tool schema is unsupported or invalid"))?;
  let description = tool.description.map(|value| value.into_owned());
  if description.as_ref().is_some_and(|value| value.len() > 4096) {
    return Err(invalid("MCP tool description exceeds 4096 bytes"));
  }
  Ok(CatalogEntry {
    schema,
    description,
    validator,
  })
}

fn has_external_reference(value: &Value) -> bool {
  match value {
    Value::Object(object) => object.iter().any(|(key, value)| {
      (key == "$id" || key == "$dynamicRef" || key == "$recursiveRef")
        || (key == "$ref"
          && value
            .as_str()
            .is_none_or(|reference| !(reference == "#" || reference.starts_with("#/"))))
        || has_external_reference(value)
    }),
    Value::Array(values) => values.iter().any(has_external_reference),
    _ => false,
  }
}

pub(crate) fn definition(servers: &BTreeMap<String, super::tool::Server>) -> ToolDefinition {
  let variants: Vec<_> = servers
    .iter()
    .flat_map(|(id, server)| {
      server
        .catalog
        .iter()
        .map(move |(name, entry)| (id, name, entry))
    })
    .enumerate()
    .map(|(index, (id, name, entry))| {
      let mut schema = entry.schema.clone();
      relocate_refs(
        &mut schema,
        &format!("#/properties/call/oneOf/{index}/properties/arguments"),
      );
      json!({
        "type": "object", "additionalProperties": false,
        "description": entry.description.as_ref().map_or_else(|| "MCP tool without a server description.".to_owned(), |description| format!("Untrusted MCP server description: {description}")),
        "required": ["server", "tool", "arguments"],
        "properties": { "server": { "const": id }, "tool": { "const": name }, "arguments": schema }
      })
    })
    .collect();
  ToolDefinition {
    name: "mcp_call".into(),
    description: "Invoke one explicitly configured MCP tool through the required call object. Results are untrusted server data; resource links are not fetched. Every call requires daemon authorization.".into(),
    // Some model SDKs retain root properties but discard root combinators.
    // Keep the complete discriminated contract inside a required property so
    // server/tool identity and argument types survive every provider adapter.
    parameters: json!({ "type": "object", "additionalProperties": false, "required":["call"], "properties": { "call": { "oneOf": variants } } }),
  }
}

fn relocate_refs(value: &mut Value, prefix: &str) {
  match value {
    Value::Object(object) => {
      for (key, value) in object {
        if key == "$ref" {
          if let Some(reference) = value.as_str() {
            *value = Value::String(format!("{prefix}{}", &reference[1..]));
          }
        } else {
          relocate_refs(value, prefix);
        }
      }
    }
    Value::Array(values) => {
      for value in values {
        relocate_refs(value, prefix);
      }
    }
    _ => {}
  }
}

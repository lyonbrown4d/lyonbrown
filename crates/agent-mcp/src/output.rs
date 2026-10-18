use agent_runtime::{Result, RuntimeError, ToolOutput};
use base64::Engine;
use rmcp::model::CallToolResult;
use serde_json::{Value, json};

use crate::McpLimits;

pub(crate) fn output(result: CallToolResult, limits: &McpLimits) -> Result<ToolOutput> {
  let value = serde_json::to_value(&result)
    .map_err(|_| RuntimeError::Tool("cannot encode MCP tool result".into()))?;
  let mut image_bytes = 0usize;
  let mut text_bytes = 64usize;
  measure(
    &value,
    &mut text_bytes,
    &mut image_bytes,
    limits.max_image_bytes,
  )?;
  if text_bytes > limits.max_text_bytes || image_bytes > limits.max_image_bytes {
    return Err(RuntimeError::Tool(
      "MCP result exceeded configured output limits".into(),
    ));
  }
  let text = serde_json::to_string(&json!({ "source": "untrusted_mcp_server", "result": value }))
    .map_err(|_| RuntimeError::Tool("cannot encode MCP result envelope".into()))?;
  Ok(ToolOutput {
    text,
    is_error: result.is_error.unwrap_or(false),
  })
}

fn measure(value: &Value, text: &mut usize, images: &mut usize, image_limit: usize) -> Result<()> {
  match value {
    Value::String(value) => {
      *text = text
        .saturating_add(serde_json::to_string(value).map_or(usize::MAX, |encoded| encoded.len()))
    }
    Value::Object(object) => {
      *text = text.saturating_add(2);
      for (key, value) in object {
        *text = text.saturating_add(
          serde_json::to_string(key)
            .map_or(usize::MAX, |encoded| encoded.len())
            .saturating_add(4),
        );
        if key == "data" && object.get("type").and_then(Value::as_str) == Some("image") {
          let encoded = value
            .as_str()
            .ok_or_else(|| RuntimeError::Tool("invalid MCP image data".into()))?;
          let mut decoded = vec![
            0;
            base64::decoded_len_estimate(encoded.len())
              .min(image_limit.saturating_sub(*images))
          ];
          let length = base64::engine::general_purpose::STANDARD
            .decode_slice(encoded, &mut decoded)
            .map_err(|_| {
              RuntimeError::Tool("MCP image is invalid or exceeds output limits".into())
            })?;
          *images = images.saturating_add(length);
        } else {
          measure(value, text, images, image_limit)?;
        }
      }
    }
    Value::Array(values) => {
      *text = text.saturating_add(values.len().saturating_add(2));
      for value in values {
        measure(value, text, images, image_limit)?;
      }
    }
    _ => *text = text.saturating_add(16),
  }
  Ok(())
}

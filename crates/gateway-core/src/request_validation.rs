use crate::GatewayError;
use serde_json::{Map, Value};

pub(crate) fn optional_fields(
  object: &Map<String, Value>,
  stream: bool,
) -> Result<(), GatewayError> {
  for key in ["parallel_tool_calls", "store", "logprobs"] {
    if object.get(key).is_some_and(|v| !v.is_boolean()) {
      return Err(GatewayError::invalid("Invalid boolean completion option"));
    }
  }
  for (key, minimum, maximum) in [
    ("temperature", 0., 2.),
    ("top_p", 0., 1.),
    ("presence_penalty", -2., 2.),
    ("frequency_penalty", -2., 2.),
  ] {
    if object.get(key).is_some_and(|v| {
      !v.as_f64()
        .is_some_and(|v| v.is_finite() && v >= minimum && v <= maximum)
    }) {
      return Err(GatewayError::invalid(
        "Completion option is outside its numeric range",
      ));
    }
  }
  for (key, minimum, maximum) in [("n", 1, 16), ("top_logprobs", 0, 20)] {
    if object
      .get(key)
      .is_some_and(|v| !v.as_u64().is_some_and(|v| v >= minimum && v <= maximum))
    {
      return Err(GatewayError::invalid("Invalid integer completion option"));
    }
  }
  for key in ["user", "service_tier", "reasoning_effort"] {
    if object.get(key).is_some_and(|v| !v.is_string()) {
      return Err(GatewayError::invalid("Invalid string completion option"));
    }
  }
  if object.get("seed").is_some_and(|v| !v.is_i64()) {
    return Err(GatewayError::invalid("seed must be an integer"));
  }
  if object.get("stop").is_some_and(|v| {
    !v.is_string()
      && !v.as_array().is_some_and(|stops| {
        !stops.is_empty() && stops.len() <= 4 && stops.iter().all(Value::is_string)
      })
  }) {
    return Err(GatewayError::invalid("Invalid stop sequence"));
  }
  if object.get("metadata").is_some_and(|v| {
    !v.as_object()
      .is_some_and(|metadata| metadata.len() <= 16 && metadata.values().all(Value::is_string))
  }) {
    return Err(GatewayError::invalid("Invalid completion metadata"));
  }
  if object.get("logit_bias").is_some_and(|v| {
    !v.as_object().is_some_and(|biases| {
      biases.values().all(|bias| {
        bias
          .as_f64()
          .is_some_and(|bias| (-100.0..=100.0).contains(&bias))
      })
    })
  }) {
    return Err(GatewayError::invalid("Invalid logit bias"));
  }
  if let Some(options) = object.get("stream_options")
    && (!stream
      || !options.as_object().is_some_and(|options| {
        options.iter().all(|(key, value)| {
          ["include_usage", "include_obfuscation"].contains(&key.as_str()) && value.is_boolean()
        })
      }))
  {
    return Err(GatewayError::invalid("Invalid stream options"));
  }
  if let Some(format) = object.get("response_format") {
    let kind = format.get("type").and_then(Value::as_str);
    if !matches!(kind, Some("text" | "json_object" | "json_schema"))
      || kind == Some("json_schema") && !format.get("json_schema").is_some_and(Value::is_object)
    {
      return Err(GatewayError::invalid("Unsupported response format"));
    }
  }
  if let Some(choice) = object.get("tool_choice")
    && !matches!(choice.as_str(), Some("none" | "auto" | "required"))
    && !(choice.get("type").and_then(Value::as_str) == Some("function")
      && choice
        .get("function")
        .and_then(|v| v.get("name"))
        .is_some_and(valid_name))
  {
    return Err(GatewayError::invalid("Invalid function tool choice"));
  }
  Ok(())
}
pub(crate) fn tools(value: Option<&Value>) -> Result<bool, GatewayError> {
  let Some(value) = value else {
    return Ok(false);
  };
  let tools = value
    .as_array()
    .filter(|tools| tools.len() <= 128)
    .ok_or_else(|| GatewayError::invalid("tools must be an array of at most 128 function tools"))?;
  for tool in tools {
    let function = tool.get("function");
    if !only_keys(tool, &["type", "function"])
      || tool.get("type").and_then(Value::as_str) != Some("function")
      || !function
        .and_then(|function| function.get("name"))
        .is_some_and(valid_name)
    {
      return Err(GatewayError::invalid(
        "Only named function tools are supported",
      ));
    }
    if let Some(function) = function.and_then(Value::as_object)
      && (function
        .keys()
        .any(|key| !["name", "description", "parameters", "strict"].contains(&key.as_str()))
        || function.get("parameters").is_some_and(|v| !v.is_object())
        || function.get("description").is_some_and(|v| !v.is_string())
        || function.get("strict").is_some_and(|v| !v.is_boolean()))
    {
      return Err(GatewayError::invalid("Invalid function tool definition"));
    }
  }
  Ok(!tools.is_empty())
}
fn valid_name(value: &Value) -> bool {
  value.as_str().is_some_and(|name| {
    !name.is_empty()
      && name.len() <= 64
      && name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
  })
}

pub(crate) fn message(message: &Value, text: &mut String) -> Result<(), GatewayError> {
  let object = message
    .as_object()
    .ok_or_else(|| GatewayError::invalid("Invalid message"))?;
  if object.keys().any(|key| {
    ![
      "role",
      "content",
      "name",
      "tool_calls",
      "tool_call_id",
      "refusal",
    ]
    .contains(&key.as_str())
  }) {
    return Err(GatewayError::invalid("Unsupported message field"));
  }
  let role = object
    .get("role")
    .and_then(Value::as_str)
    .ok_or_else(|| GatewayError::invalid("Message role is required"))?;
  if !["system", "developer", "user", "assistant", "tool"].contains(&role) {
    return Err(GatewayError::invalid("Unsupported message role"));
  }
  if object.get("name").is_some_and(|name| !valid_name(name)) {
    return Err(GatewayError::invalid("Invalid message name"));
  }
  match object.get("content") {
    Some(Value::String(content)) => {
      text.push_str(content);
      text.push('\n');
    }
    Some(Value::Array(parts)) => {
      for part in parts {
        if !only_keys(part, &["type", "text"])
          || part.get("type").and_then(Value::as_str) != Some("text")
          || part.get("text").and_then(Value::as_str).is_none()
        {
          return Err(GatewayError::invalid(
            "Only text message content is supported",
          ));
        }
        if let Some(content) = part.get("text").and_then(Value::as_str) {
          text.push_str(content);
          text.push('\n');
        }
      }
    }
    None | Some(Value::Null)
      if role == "assistant"
        && (object.contains_key("tool_calls")
          || object.get("refusal").is_some_and(Value::is_string)) => {}
    _ => return Err(GatewayError::invalid("Text content is required")),
  }
  if role == "tool"
    && !object
      .get("tool_call_id")
      .and_then(Value::as_str)
      .is_some_and(|id| !id.is_empty())
  {
    return Err(GatewayError::invalid("Tool result requires tool_call_id"));
  }
  if let Some(calls) = object.get("tool_calls")
    && (role != "assistant"
      || !calls.as_array().is_some_and(|calls| {
        !calls.is_empty()
          && calls.len() <= 128
          && calls.iter().all(|call| {
            call
              .get("id")
              .and_then(Value::as_str)
              .is_some_and(|id| !id.is_empty())
              && call.get("type").and_then(Value::as_str) == Some("function")
              && call.get("function").is_some_and(|function| {
                function.get("name").is_some_and(valid_name)
                  && function.get("arguments").is_some_and(Value::is_string)
              })
          })
      }))
  {
    return Err(GatewayError::invalid("Invalid function tool calls"));
  }
  Ok(())
}

fn only_keys(value: &Value, keys: &[&str]) -> bool {
  value
    .as_object()
    .is_some_and(|object| object.keys().all(|key| keys.contains(&key.as_str())))
}

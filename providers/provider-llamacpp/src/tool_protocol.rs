use super::*;

pub(super) const MAX_TOOL_CALL_BYTES: usize = 64 * 1024;
pub(super) const MAX_TOOL_GUIDANCE_BYTES: usize = 32 * 1024;
const OPEN: &str = "<tool_call>";
const CLOSE: &str = "</tool_call>";
pub(super) const MAX_BUFFERED_OUTPUT_BYTES: usize = MAX_TOOL_CALL_BYTES + OPEN.len() + CLOSE.len();

#[derive(Debug, PartialEq)]
pub(super) enum ParsedModelOutput {
  Text(String),
  ToolCall(ToolInvocation),
}

pub(super) fn tool_protocol_guidance(tools: &[ToolDefinition]) -> Result<String, ModelError> {
  let definitions = tools
    .iter()
    .map(|tool| {
      serde_json::json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.input_schema,
      })
    })
    .collect::<Vec<_>>();
  let definitions = serde_json::to_string(&definitions)
    .map_err(|error| ModelError::Protocol(format!("cannot serialize tools: {error}")))?;
  let guidance = format!(
    "Available tools: {definitions}\nWhen a tool is required, output exactly one envelope and no other text: <tool_call>{{\"name\":\"tool_name\",\"arguments\":{{...}}}}</tool_call>. Use only a listed tool and arguments matching its input_schema. Otherwise answer normally."
  );
  if guidance.len() > MAX_TOOL_GUIDANCE_BYTES {
    return Err(ModelError::Protocol(format!(
      "tool definitions exceed {MAX_TOOL_GUIDANCE_BYTES} bytes"
    )));
  }
  Ok(guidance)
}

pub(super) fn format_tool_call(call: &ToolInvocation) -> Result<String, ModelError> {
  let payload = serde_json::to_string(&serde_json::json!({
    "name": call.name,
    "arguments": call.arguments,
  }))
  .map_err(|error| ModelError::Protocol(format!("cannot serialize tool call: {error}")))?;
  Ok(format!("{OPEN}{payload}{CLOSE}"))
}

pub(super) fn append_buffered_output(output: &mut String, delta: &str) -> Result<(), ModelError> {
  let new_len = output
    .len()
    .checked_add(delta.len())
    .ok_or_else(|| ModelError::Protocol("buffered tool output length overflowed".into()))?;
  if new_len > MAX_BUFFERED_OUTPUT_BYTES {
    return Err(ModelError::Protocol(format!(
      "buffered tool output exceeds {MAX_BUFFERED_OUTPUT_BYTES} bytes"
    )));
  }
  output.push_str(delta);
  Ok(())
}

pub(super) fn parse_model_output(
  output: &str,
  tools: &[ToolDefinition],
) -> Result<ParsedModelOutput, ModelError> {
  let trimmed = output.trim();
  if !trimmed.contains(OPEN) && !trimmed.contains(CLOSE) {
    return Ok(ParsedModelOutput::Text(output.to_owned()));
  }
  let payload = trimmed
    .strip_prefix(OPEN)
    .and_then(|value| value.strip_suffix(CLOSE))
    .ok_or_else(|| ModelError::Protocol("malformed tool-call envelope".into()))?;
  if payload.len() > MAX_TOOL_CALL_BYTES {
    return Err(ModelError::Protocol(format!(
      "tool call exceeds {MAX_TOOL_CALL_BYTES} bytes"
    )));
  }
  let value: serde_json::Value = serde_json::from_str(payload)
    .map_err(|error| ModelError::Protocol(format!("invalid tool-call JSON: {error}")))?;
  let object = value
    .as_object()
    .ok_or_else(|| ModelError::Protocol("tool-call payload must be an object".into()))?;
  if object.len() != 2 || !object.contains_key("name") || !object.contains_key("arguments") {
    return Err(ModelError::Protocol(
      "tool-call payload must contain only name and arguments".into(),
    ));
  }
  let name = object["name"]
    .as_str()
    .filter(|name| !name.is_empty())
    .ok_or_else(|| ModelError::Protocol("tool-call name must be a non-empty string".into()))?;
  let tool = tools
    .iter()
    .find(|tool| tool.name == name)
    .ok_or_else(|| ModelError::Protocol(format!("unknown tool {name:?}")))?;
  let arguments = object["arguments"].clone();
  if !arguments.is_object() {
    return Err(ModelError::Protocol(
      "tool-call arguments must be an object".into(),
    ));
  }
  let validator = jsonschema::validator_for(&tool.input_schema)
    .map_err(|error| ModelError::Protocol(format!("invalid tool schema: {error}")))?;
  if let Some(error) = validator.iter_errors(&arguments).next() {
    return Err(ModelError::Protocol(format!(
      "tool arguments do not match schema: {error}"
    )));
  }
  Ok(ParsedModelOutput::ToolCall(ToolInvocation {
    id: ToolCallId::new(),
    name: name.to_owned(),
    arguments,
  }))
}

#[cfg(feature = "llama-cpp")]
pub(super) fn emit_buffered_output(
  output: &str,
  tools: &[ToolDefinition],
  sender: &mpsc::Sender<Result<ModelEvent, ModelError>>,
  text_stop_reason: StopReason,
) -> Result<Option<StopReason>, ModelError> {
  let event = match parse_model_output(output, tools)? {
    ParsedModelOutput::Text(text) if text.is_empty() => return Ok(Some(text_stop_reason)),
    ParsedModelOutput::Text(text) => ModelEvent::Delta(text),
    ParsedModelOutput::ToolCall(call) => ModelEvent::ToolCall(call),
  };
  let stop_reason = if matches!(event, ModelEvent::ToolCall(_)) {
    StopReason::ToolUse
  } else {
    text_stop_reason
  };
  Ok(
    sender
      .blocking_send(Ok(event))
      .is_ok()
      .then_some(stop_reason),
  )
}

pub(super) fn apply_qwen3_thinking_control(
  template: &str,
  messages: &mut [PromptMessage],
  thinking: Option<bool>,
) {
  if thinking != Some(false)
    || !template.contains("enable_thinking")
    || !template.contains("<think>")
  {
    return;
  }
  if let Some(message) = messages
    .iter_mut()
    .rev()
    .find(|message| message.role == "user")
    && !message.content.ends_with("/no_think")
  {
    message.content.push_str(" /no_think");
  }
}

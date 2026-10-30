use crate::TokenUsage;
use serde_json::Value;
use std::io;

/// Read OpenAI-compatible chat usage metadata without changing the forwarded payload.
pub(crate) fn from_response(value: &Value) -> Result<Option<TokenUsage>, io::Error> {
  from_object_field(value, "usage")
}

pub(crate) fn from_sse_event(value: &Value) -> Result<Option<TokenUsage>, io::Error> {
  from_object_field(value, "usage")
}

fn from_object_field(value: &Value, field: &str) -> Result<Option<TokenUsage>, io::Error> {
  let Some(usage) = value.get(field) else {
    return Ok(None);
  };
  if usage.is_null() {
    return Ok(None);
  }
  let object = usage
    .as_object()
    .ok_or_else(|| invalid("usage metadata is not an object"))?;
  let input_tokens = required_count(object, "prompt_tokens")?;
  let output_tokens = required_count(object, "completion_tokens")?;
  let cached_input_tokens =
    nested_optional_count(object, "prompt_tokens_details", "cached_tokens")?;
  let reasoning_tokens =
    nested_optional_count(object, "completion_tokens_details", "reasoning_tokens")?;
  if cached_input_tokens > input_tokens || reasoning_tokens > output_tokens {
    return Err(invalid("usage detail count exceeds its total token count"));
  }
  input_tokens
    .checked_add(output_tokens)
    .ok_or_else(|| invalid("combined usage token count is out of range"))?;
  Ok(Some(TokenUsage {
    input_tokens,
    output_tokens,
    cached_input_tokens,
    reasoning_tokens,
  }))
}

fn required_count(object: &serde_json::Map<String, Value>, field: &str) -> Result<u64, io::Error> {
  object
    .get(field)
    .and_then(Value::as_u64)
    .ok_or_else(|| invalid("usage token count is missing, negative, or out of range"))
}

fn nested_optional_count(
  object: &serde_json::Map<String, Value>,
  parent: &str,
  field: &str,
) -> Result<u64, io::Error> {
  let Some(details) = object.get(parent).filter(|details| !details.is_null()) else {
    return Ok(0);
  };
  let details = details
    .as_object()
    .ok_or_else(|| invalid("usage details are not an object"))?;
  match details.get(field) {
    Some(Value::Null) | None => Ok(0),
    Some(value) => value
      .as_u64()
      .ok_or_else(|| invalid("usage token count is missing, negative, or out of range")),
  }
}

fn invalid(message: &'static str) -> io::Error {
  io::Error::other(message)
}

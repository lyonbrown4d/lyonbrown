use crate::GatewayError;
use decision_core::ModelTier;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Gateway-specific restrictions are removed before transfer to the provider.
#[derive(Clone, Debug, Default, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RoutingConstraints {
  pub local_only: bool,
  #[schema(value_type = Vec<String>)]
  pub allowed_tiers: BTreeSet<ModelTier>,
}

/// Text and function tool messages use the OpenAI Chat Completions wire representation.
#[derive(utoipa::ToSchema)]
pub struct ChatRequestSchema {
  pub model: String,
  pub messages: Vec<Value>,
  pub stream: Option<bool>,
  pub tools: Option<Vec<Value>>,
  pub tool_choice: Option<Value>,
  pub max_completion_tokens: Option<u64>,
  pub routing: Option<RoutingConstraints>,
  pub stream_options: Option<Value>,
  pub parallel_tool_calls: Option<bool>,
  pub max_tokens: Option<u64>,
  pub temperature: Option<f64>,
  pub top_p: Option<f64>,
  #[schema(minimum = 1, maximum = 16)]
  pub n: Option<u64>,
  pub stop: Option<Value>,
  pub presence_penalty: Option<f64>,
  pub frequency_penalty: Option<f64>,
  pub logit_bias: Option<Value>,
  pub logprobs: Option<bool>,
  pub top_logprobs: Option<u64>,
  pub response_format: Option<Value>,
  pub seed: Option<i64>,
  pub user: Option<String>,
  pub store: Option<bool>,
  pub metadata: Option<Value>,
  pub service_tier: Option<String>,
  pub reasoning_effort: Option<String>,
}

pub(crate) struct ValidatedRequest {
  pub body: Value,
  pub requested_model: String,
  pub text: String,
  pub stream: bool,
  pub tools: bool,
  pub context_tokens: u64,
  pub reserved_tokens: u64,
  pub routing: RoutingConstraints,
}
impl ValidatedRequest {
  pub fn parse(mut body: Value, default_output: u64) -> Result<Self, GatewayError> {
    let object = body
      .as_object_mut()
      .ok_or_else(|| GatewayError::invalid("Expected a JSON object"))?;
    const FIELDS: &[&str] = &[
      "model",
      "messages",
      "stream",
      "stream_options",
      "tools",
      "tool_choice",
      "parallel_tool_calls",
      "max_tokens",
      "max_completion_tokens",
      "temperature",
      "top_p",
      "n",
      "stop",
      "presence_penalty",
      "frequency_penalty",
      "logit_bias",
      "logprobs",
      "top_logprobs",
      "response_format",
      "seed",
      "user",
      "routing",
      "store",
      "metadata",
      "service_tier",
      "reasoning_effort",
    ];
    if object.keys().any(|key| !FIELDS.contains(&key.as_str())) {
      return Err(GatewayError::invalid("Unsupported Chat Completions field"));
    }
    let requested_model = object
      .get("model")
      .and_then(Value::as_str)
      .filter(|s| !s.is_empty())
      .ok_or_else(|| GatewayError::invalid("model is required"))?
      .to_owned();
    let stream = match object.get("stream") {
      None => false,
      Some(Value::Bool(b)) => *b,
      _ => return Err(GatewayError::invalid("stream must be boolean")),
    };
    let messages = object
      .get("messages")
      .and_then(Value::as_array)
      .filter(|m| !m.is_empty())
      .ok_or_else(|| GatewayError::invalid("messages must be a nonempty array"))?;
    let mut text = String::new();
    for message in messages {
      crate::request_validation::message(message, &mut text)?;
    }
    crate::request_validation::optional_fields(object, stream)?;
    let tools = crate::request_validation::tools(object.get("tools"))?;
    let routing = object
      .remove("routing")
      .map(serde_json::from_value)
      .transpose()
      .map_err(|_| GatewayError::invalid("Invalid routing constraints"))?
      .unwrap_or_default();
    if object.contains_key("max_tokens") && object.contains_key("max_completion_tokens") {
      return Err(GatewayError::invalid("Supply only one output token limit"));
    }
    let output = object
      .get("max_completion_tokens")
      .or_else(|| object.get("max_tokens"))
      .map(|n| {
        n.as_u64()
          .filter(|n| *n > 0)
          .ok_or_else(|| GatewayError::invalid("Output token limit must be positive"))
      })
      .transpose()?
      .unwrap_or(default_output);
    if !object.contains_key("max_tokens") && !object.contains_key("max_completion_tokens") {
      object.insert("max_tokens".into(), output.into());
    }
    // A conservative UTF-8 byte budget includes message/tool JSON overhead and reserved output.
    let reserved_output = output
      .checked_mul(body.get("n").and_then(Value::as_u64).unwrap_or(1))
      .ok_or_else(|| GatewayError::invalid("Output token reservation overflow"))?;
    let context_tokens = (body.to_string().len() as u64)
      .checked_add(output)
      .ok_or_else(|| GatewayError::invalid("Context limit overflow"))?;
    let reserved_tokens = (body.to_string().len() as u64)
      .checked_add(reserved_output)
      .ok_or_else(|| GatewayError::invalid("Token reservation overflow"))?;
    Ok(Self {
      body,
      requested_model,
      text,
      stream,
      tools,
      context_tokens,
      reserved_tokens,
      routing,
    })
  }
}

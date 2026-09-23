use agent_model::{ChatMessage, ModelRequest, Role};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
pub(crate) struct ChatCompletionRequest {
  model: String,
  messages: Vec<WireMessage>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  tools: Vec<WireTool>,
  stream: bool,
  stream_options: WireStreamOptions,
  max_tokens: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  temperature: Option<f32>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  stop: Vec<String>,
}

#[derive(Serialize)]
struct WireMessage {
  role: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  content: Option<String>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  tool_calls: Vec<WireToolCall>,
  #[serde(skip_serializing_if = "Option::is_none")]
  tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct WireTool {
  #[serde(rename = "type")]
  kind: &'static str,
  function: WireFunctionDefinition,
}

#[derive(Serialize)]
struct WireFunctionDefinition {
  name: String,
  description: String,
  parameters: Value,
}

#[derive(Serialize)]
struct WireToolCall {
  id: String,
  #[serde(rename = "type")]
  kind: &'static str,
  function: WireFunctionCall,
}

#[derive(Serialize)]
struct WireFunctionCall {
  name: String,
  arguments: String,
}

#[derive(Serialize)]
struct WireStreamOptions {
  include_usage: bool,
}

#[derive(Deserialize)]
struct CompletionChunk {
  #[serde(default)]
  choices: Vec<WireChoice>,
  usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
  delta: WireDelta,
  finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct WireDelta {
  #[serde(default)]
  content: Option<String>,
  #[serde(default)]
  tool_calls: Vec<WireDeltaToolCall>,
}

#[derive(Deserialize)]
struct WireDeltaToolCall {
  index: u64,
  function: Option<WireDeltaFunction>,
}

#[derive(Deserialize, Default)]
struct WireDeltaFunction {
  name: Option<String>,
  arguments: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
  #[serde(default)]
  prompt_tokens: u32,
  #[serde(default)]
  completion_tokens: u32,
}

pub(crate) fn to_wire_request(request: ModelRequest) -> ChatCompletionRequest {
  let mut messages: Vec<WireMessage> = Vec::with_capacity(request.messages.len() + 1);
  if let Some(system) = request.system {
    messages.push(WireMessage {
      role: "system".into(),
      content: Some(system),
      tool_calls: vec![],
      tool_call_id: None,
    });
  }
  messages.extend(request.messages.into_iter().map(to_wire_message));
  ChatCompletionRequest {
    model: request.model,
    messages,
    tools: request
      .tools
      .into_iter()
      .map(|tool| WireTool {
        kind: "function",
        function: WireFunctionDefinition {
          name: tool.name,
          description: tool.description,
          parameters: tool.input_schema,
        },
      })
      .collect(),
    stream: true,
    stream_options: WireStreamOptions {
      include_usage: true,
    },
    max_tokens: request.max_tokens,
    temperature: request.temperature,
    stop: request.stop,
  }
}

fn to_wire_message(message: ChatMessage) -> WireMessage {
  match message.role {
    Role::System | Role::User => WireMessage {
      role: match message.role {
        Role::System => "system".into(),
        _ => "user".into(),
      },
      content: Some(message.content),
      tool_calls: vec![],
      tool_call_id: None,
    },
    Role::Assistant => WireMessage {
      role: "assistant".into(),
      content: (!message.content.is_empty()).then_some(message.content),
      tool_calls: message
        .tool_calls
        .into_iter()
        .map(|call| WireToolCall {
          id: call.id.as_uuid().to_string(),
          kind: "function",
          function: WireFunctionCall {
            name: call.name,
            arguments: call.arguments.to_string(),
          },
        })
        .collect(),
      tool_call_id: None,
    },
    Role::Tool => WireMessage {
      role: "tool".into(),
      content: Some(message.content),
      tool_calls: vec![],
      tool_call_id: message.tool_call_id.map(|id| id.as_uuid().to_string()),
    },
  }
}

mod parser;
pub(crate) use parser::into_model_stream;

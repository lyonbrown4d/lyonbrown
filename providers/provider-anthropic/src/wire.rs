use agent_model::{ModelRequest, Role};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize)]
pub(crate) struct MessagesRequest {
  model: String,
  max_tokens: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  system: Option<String>,
  messages: Vec<WireMessage>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  tools: Vec<WireTool>,
  stream: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  temperature: Option<f32>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  stop_sequences: Vec<String>,
}

#[derive(Serialize)]
struct WireMessage {
  role: String,
  content: Value,
}

#[derive(Serialize)]
struct WireTool {
  name: String,
  description: String,
  input_schema: Value,
}

pub(crate) fn to_wire_request(request: ModelRequest) -> MessagesRequest {
  let mut system = request.system.unwrap_or_default();
  let mut messages: Vec<(String, Value)> = Vec::new();
  for message in request.messages {
    match message.role {
      Role::System => {
        system = if system.is_empty() {
          message.content
        } else {
          format!("{system}\n\n{}", message.content)
        };
      }
      Role::User => messages.push(("user".into(), Value::String(message.content))),
      Role::Assistant => {
        let mut blocks = Vec::new();
        if !message.content.is_empty() {
          blocks.push(json!({"type": "text", "text": message.content}));
        }
        for call in message.tool_calls {
          blocks.push(json!({
              "type": "tool_use",
              "id": call.id.as_uuid().to_string(),
              "name": call.name,
              "input": call.arguments,
          }));
        }
        messages.push(("assistant".into(), Value::Array(blocks)));
      }
      Role::Tool => {
        let tool_use_id = message
          .tool_call_id
          .map_or_else(String::new, |id| id.as_uuid().to_string());
        messages.push((
          "user".into(),
          json!({
              "type": "tool_result",
              "tool_use_id": tool_use_id,
              "content": message.content,
          }),
        ));
      }
    }
  }
  let merged = merge_consecutive_roles(messages);
  MessagesRequest {
    model: request.model,
    max_tokens: request.max_tokens,
    system: (!system.is_empty()).then_some(system),
    messages: merged,
    tools: request
      .tools
      .into_iter()
      .map(|tool| WireTool {
        name: tool.name,
        description: tool.description,
        input_schema: tool.input_schema,
      })
      .collect(),
    stream: true,
    temperature: request.temperature,
    stop_sequences: request.stop,
  }
}

fn merge_consecutive_roles(messages: Vec<(String, Value)>) -> Vec<WireMessage> {
  let mut merged: Vec<WireMessage> = Vec::new();
  for (role, content) in messages {
    match merged.last_mut() {
      Some(last) if last.role == role => {
        last.content = merge_content(std::mem::take(&mut last.content), content)
      }
      _ => merged.push(WireMessage { role, content }),
    }
  }
  merged
}

fn merge_content(existing: Value, next: Value) -> Value {
  match (existing, next) {
    (Value::String(a), Value::String(b)) => Value::String(format!("{a}\n\n{b}")),
    (Value::Array(mut a), Value::Array(b)) => {
      a.extend(b);
      Value::Array(a)
    }
    (Value::String(a), Value::Array(mut b)) => {
      b.insert(0, json!({"type": "text", "text": a}));
      Value::Array(b)
    }
    (Value::Array(mut a), Value::String(b)) => {
      a.push(json!({"type": "text", "text": b}));
      Value::Array(a)
    }
    (a, b) => Value::Array(vec![a, b]),
  }
}

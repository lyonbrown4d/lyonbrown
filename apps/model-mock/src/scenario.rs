use agent_test_support::{ScriptedReply, StandardProtocol, text_reply, tool_reply};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
  ChatCompletions,
  Responses,
  Anthropic,
  OllamaChat,
  OllamaGenerate,
}
impl Protocol {
  pub fn standard(self) -> Option<StandardProtocol> {
    match self {
      Self::ChatCompletions => Some(StandardProtocol::ChatCompletions),
      Self::Responses => Some(StandardProtocol::Responses),
      Self::Anthropic => Some(StandardProtocol::Anthropic),
      Self::OllamaChat | Self::OllamaGenerate => None,
    }
  }
  pub fn is_ollama(self) -> bool {
    self.standard().is_none()
  }
  pub fn from_path(path: &str) -> Option<Self> {
    match path {
      "/v1/chat/completions" => Some(Self::ChatCompletions),
      "/v1/responses" => Some(Self::Responses),
      "/v1/messages" => Some(Self::Anthropic),
      "/api/chat" => Some(Self::OllamaChat),
      "/api/generate" => Some(Self::OllamaGenerate),
      _ => None,
    }
  }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
  pub model: String,
  pub protocol: Protocol,
  #[serde(default)]
  pub repeat_last: bool,
  pub replies: Vec<Reply>,
}
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
  Text {
    text: String,
    #[serde(default)]
    delay_ms: u64,
  },
  Tool {
    name: String,
    arguments: Value,
    #[serde(default)]
    delay_ms: u64,
  },
  Error {
    status: u16,
    #[serde(default)]
    delay_ms: u64,
  },
  /// An arbitrary assistant text, such as invalid JSON or low-confidence output.
  Decision {
    text: String,
    #[serde(default)]
    delay_ms: u64,
  },
  Stall,
  Truncated {
    text: String,
  },
  MissingTerminal {
    text: String,
  },
  Malformed {
    body: String,
  },
}
impl Reply {
  pub fn delay_ms(&self) -> u64 {
    match self {
      Self::Text { delay_ms, .. }
      | Self::Tool { delay_ms, .. }
      | Self::Error { delay_ms, .. }
      | Self::Decision { delay_ms, .. } => *delay_ms,
      Self::Stall
      | Self::Truncated { .. }
      | Self::MissingTerminal { .. }
      | Self::Malformed { .. } => 0,
    }
  }
  pub fn scripted(&self, protocol: Protocol, model: &str, streaming: bool) -> ScriptedReply {
    let Some(standard) = protocol.standard() else {
      return crate::ollama::scripted(self, protocol, model, streaming);
    };
    match self {
      Self::Text { text, .. } | Self::Decision { text, .. } if !streaming => {
        ScriptedReply::status(200, json_reply(protocol, text, None))
      }
      Self::Text { text, .. } | Self::Decision { text, .. } => text_reply(standard, text),
      Self::Tool {
        name, arguments, ..
      } if !streaming => {
        ScriptedReply::status(200, json_reply(protocol, "", Some((name, arguments))))
      }
      Self::Tool {
        name, arguments, ..
      } => tool_reply(standard, name, arguments.clone()),
      Self::Error { status, .. } => ScriptedReply::status(
        *status,
        json!({"error":{"type":"synthetic_error","message":"synthetic model failure"}}),
      ),
      Self::Stall => ScriptedReply::stall(),
      Self::Truncated { .. } => agent_test_support::truncated_reply(standard),
      Self::MissingTerminal { .. } => agent_test_support::truncated_reply(standard).stall_after(),
      Self::Malformed { body } => ScriptedReply::raw_sse(body.as_bytes().to_vec()),
    }
  }
}
fn json_reply(protocol: Protocol, text: &str, tool: Option<(&str, &Value)>) -> Value {
  match protocol {
    Protocol::ChatCompletions => {
      let message = if let Some((name, arguments)) = tool {
        json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_fixture","type":"function","function":{"name":name,"arguments":arguments.to_string()}}]})
      } else {
        json!({"role":"assistant","content":text})
      };
      json!({"id":"chat_fixture","object":"chat.completion","model":"synthetic","choices":[{"index":0,"message":message,"finish_reason":if tool.is_some(){"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":11,"completion_tokens":3,"total_tokens":14}})
    }
    Protocol::Responses => {
      let item = if let Some((name, arguments)) = tool {
        json!({"type":"function_call","id":"fc_fixture","call_id":"call_fixture","name":name,"arguments":arguments.to_string()})
      } else {
        json!({"type":"message","id":"msg_fixture","role":"assistant","content":[{"type":"output_text","text":text}]})
      };
      json!({"id":"response_fixture","object":"response","status":"completed","output":[item],"usage":{"input_tokens":11,"output_tokens":3,"total_tokens":14}})
    }
    Protocol::OllamaChat | Protocol::OllamaGenerate => {
      crate::ollama::json_reply(protocol, "synthetic", text, tool, true)
    }
    Protocol::Anthropic => {
      let block = if let Some((name, arguments)) = tool {
        json!({"type":"tool_use","id":"call_fixture","name":name,"input":arguments})
      } else {
        json!({"type":"text","text":text})
      };
      json!({"id":"message_fixture","type":"message","role":"assistant","model":"synthetic","content":[block],"stop_reason":if tool.is_some(){"tool_use"}else{"end_turn"},"usage":{"input_tokens":11,"output_tokens":3}})
    }
  }
}

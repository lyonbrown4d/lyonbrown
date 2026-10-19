use crate::ScriptedReply;
use serde_json::{Value, json};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardProtocol {
  ChatCompletions,
  Responses,
  Anthropic,
}
impl StandardProtocol {
  pub const ALL: [Self; 3] = [Self::ChatCompletions, Self::Responses, Self::Anthropic];
  pub fn provider(self) -> &'static str {
    match self {
      Self::ChatCompletions => "openai-completions",
      Self::Responses => "openai-responses",
      Self::Anthropic => "anthropic",
    }
  }
  pub fn path(self) -> &'static str {
    match self {
      Self::ChatCompletions => "/v1/chat/completions",
      Self::Responses => "/v1/responses",
      Self::Anthropic => "/v1/messages",
    }
  }
}
pub fn text_reply(protocol: StandardProtocol, text: &str) -> ScriptedReply {
  match protocol {
    StandardProtocol::ChatCompletions => chat_text(text),
    StandardProtocol::Responses => responses_text(text),
    StandardProtocol::Anthropic => anthropic_text(text),
  }
  .fragmented(7)
}
pub fn tool_reply(protocol: StandardProtocol, name: &str, arguments: Value) -> ScriptedReply {
  tool_reply_with_id(protocol, "call_fixture", name, arguments)
}
/// Distinct provider call IDs let a single Harness run perform multiple durable tools.
pub fn tool_reply_with_id(
  protocol: StandardProtocol,
  call_id: &str,
  name: &str,
  arguments: Value,
) -> ScriptedReply {
  match protocol {
    StandardProtocol::ChatCompletions => chat_tool(call_id, name, arguments),
    StandardProtocol::Responses => responses_tool(call_id, name, arguments),
    StandardProtocol::Anthropic => anthropic_tool(call_id, name, arguments),
  }
  .fragmented(5)
}
fn halves(value: &str) -> (&str, &str) {
  let mut middle = value.len() / 2;
  while !value.is_char_boundary(middle) {
    middle -= 1;
  }
  value.split_at(middle)
}
fn chat_text(text: &str) -> ScriptedReply {
  let (first, last) = halves(text);
  ScriptedReply::sse(vec![
    json!({"choices":[{"delta":{"role":"assistant","content":first},"finish_reason":null}]}),
    json!({"choices":[{"delta":{"content":last},"finish_reason":null}]}),
    json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":11,"completion_tokens":3,"total_tokens":14}}),
  ])
  .with_sse_done()
}
fn chat_tool(call_id: &str, name: &str, arguments: Value) -> ScriptedReply {
  let encoded = arguments.to_string();
  let (first, last) = halves(&encoded);
  ScriptedReply::sse(vec![
    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":call_id,"type":"function","function":{"name":name,"arguments":first}}]},"finish_reason":null}]}),
    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":last}}]},"finish_reason":null}]}),
    json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}}),
  ])
  .with_sse_done()
}
fn responses_text(text: &str) -> ScriptedReply {
  let (first, last) = halves(text);
  let item = json!({"type":"message","id":"msg_fixture","role":"assistant","content":[{"type":"output_text","text":text}]});
  ScriptedReply::sse(vec![
    json!({"type":"response.created","response":{"id":"response_fixture","status":"in_progress"}}),
    json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_fixture","role":"assistant","content":[]}}),
    json!({"type":"response.content_part.added","output_index":0,"content_index":0,"item_id":"msg_fixture","part":{"type":"output_text","text":""}}),
    json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"msg_fixture","delta":first}),
    json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"msg_fixture","delta":last}),
    json!({"type":"response.output_text.done","output_index":0,"content_index":0,"item_id":"msg_fixture","text":text}),
    json!({"type":"response.output_item.done","output_index":0,"item":item}),
    json!({"type":"response.completed","response":{"id":"response_fixture","status":"completed","output":[item],"usage":{"input_tokens":11,"output_tokens":3,"total_tokens":14}}}),
  ])
}
fn responses_tool(call_id: &str, name: &str, arguments: Value) -> ScriptedReply {
  let encoded = arguments.to_string();
  let (first, last) = halves(&encoded);
  let item = json!({"type":"function_call","id":"fc_fixture","call_id":call_id,"name":name,"arguments":encoded});
  ScriptedReply::sse(vec![
    json!({"type":"response.created","response":{"id":"response_fixture","status":"in_progress"}}),
    json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_fixture","call_id":call_id,"name":name,"arguments":""}}),
    json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_fixture","delta":first}),
    json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_fixture","delta":last}),
    json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_fixture","arguments":encoded}),
    json!({"type":"response.output_item.done","output_index":0,"item":item}),
    json!({"type":"response.completed","response":{"id":"response_fixture","status":"completed","output":[item],"usage":{"input_tokens":7,"output_tokens":2,"total_tokens":9}}}),
  ])
}
fn anthropic_start(input: u64) -> Value {
  json!({"type":"message_start","message":{"id":"message_fixture","type":"message","role":"assistant","model":"fixture-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":input,"output_tokens":0}}})
}
fn anthropic_text(text: &str) -> ScriptedReply {
  let (first, last) = halves(text);
  ScriptedReply::named_sse(vec![
    ("message_start".into(), anthropic_start(11)),
    (
      "content_block_start".into(),
      json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
    ),
    (
      "content_block_delta".into(),
      json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":first}}),
    ),
    (
      "content_block_delta".into(),
      json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":last}}),
    ),
    (
      "content_block_stop".into(),
      json!({"type":"content_block_stop","index":0}),
    ),
    (
      "message_delta".into(),
      json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}),
    ),
    ("message_stop".into(), json!({"type":"message_stop"})),
  ])
}
fn anthropic_tool(call_id: &str, name: &str, arguments: Value) -> ScriptedReply {
  let encoded = arguments.to_string();
  let (first, last) = halves(&encoded);
  ScriptedReply::named_sse(vec![
    ("message_start".into(), anthropic_start(7)),
    (
      "content_block_start".into(),
      json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":call_id,"name":name,"input":{}}}),
    ),
    (
      "content_block_delta".into(),
      json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":first}}),
    ),
    (
      "content_block_delta".into(),
      json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":last}}),
    ),
    (
      "content_block_stop".into(),
      json!({"type":"content_block_stop","index":0}),
    ),
    (
      "message_delta".into(),
      json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":2}}),
    ),
    ("message_stop".into(), json!({"type":"message_stop"})),
  ])
}
pub fn truncated_reply(protocol: StandardProtocol) -> ScriptedReply {
  match protocol {
    StandardProtocol::ChatCompletions => ScriptedReply::sse(vec![
      json!({"choices":[{"delta":{"content":"unfinished"},"finish_reason":null}]}),
    ]),
    StandardProtocol::Responses => ScriptedReply::sse(vec![
      json!({"type":"response.created","response":{"id":"unfinished","status":"in_progress"}}),
    ]),
    StandardProtocol::Anthropic => {
      ScriptedReply::named_sse(vec![("message_start".into(), anthropic_start(1))])
    }
  }
}

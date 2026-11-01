use crate::{ChatMessage, GenerationRequest, InputContent, MessageRole, ToolCall, ToolDefinition};
use either::Either;
use mistralrs::{RequestLike, RequestMessage};
use serde_json::json;

#[test]
fn sdk_request_retains_structured_assistant_call_and_tool_result_identity() {
  let arguments = json!({"path":"notes.txt"});
  let mut request = crate::mistral::build_request(GenerationRequest {
    messages: vec![
      ChatMessage {
        role: MessageRole::Assistant,
        content: vec![],
        tool_calls: vec![ToolCall {
          id: "call-1".into(),
          name: "read_file".into(),
          arguments: arguments.clone(),
        }],
        tool_call_id: None,
      },
      ChatMessage {
        role: MessageRole::Tool,
        content: vec![InputContent::Text {
          text: "actual marker".into(),
        }],
        tool_calls: vec![],
        tool_call_id: Some("call-1".into()),
      },
    ],
    tools: vec![ToolDefinition {
      name: "read_file".into(),
      description: "Read a file".into(),
      parameters: json!({"type":"object"}),
    }],
    max_tokens: 64,
    temperature: Some(0.0),
    seed: 0,
  })
  .expect("canonical SDK request");
  let calls = request.messages_ref()[0]["tool_calls"]
    .as_ref()
    .right()
    .expect("structured calls");
  assert_eq!(calls[0]["id"], "call-1");
  assert_eq!(calls[0]["function"]["name"], "read_file");
  assert_eq!(calls[0]["function"]["arguments"], arguments);
  assert!(!request.messages_ref()[0].contains_key("function"));
  assert_eq!(
    request.messages_ref()[1]["tool_call_id"],
    Either::Left("call-1".into())
  );
  assert!(request.take_tools().is_some());
  let RequestMessage::Chat { messages, .. } = request.take_messages() else {
    panic!("text chat required");
  };
  assert_eq!(messages[1]["content"], Either::Left("actual marker".into()));
  assert!(messages[0].contains_key("tool_calls"));
}

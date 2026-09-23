#[path = "../src/session.rs"]
mod session_impl;

use agent_core::{AgentEvent, ContextItemKind, ToolInvocation};
use agent_model::{ChatMessage, Role};
use serde_json::json;
use session_impl::Session;

fn message(role: Role, content: &str) -> ChatMessage {
  ChatMessage {
    role,
    content: content.into(),
    tool_call_id: None,
    tool_name: None,
    tool_calls: vec![],
  }
}

#[test]
fn duplicate_message_text_is_selected_by_context_identity() {
  let mut session = Session::new();
  let old = session.push_message(
    message(Role::User, "duplicate"),
    ContextItemKind::Message,
    "user",
  );
  let new = session.push_message(
    message(Role::User, "duplicate"),
    ContextItemKind::Message,
    "user",
  );

  let selected = session.selected_messages(std::slice::from_ref(&new));

  assert_eq!(selected.len(), 1);
  assert_eq!(selected[0].content, "duplicate");
  assert_ne!(old.id, new.id);
  assert_eq!(session.budget_items().len(), 2);
}

#[test]
fn assistant_tool_call_is_retained_only_with_selected_observation() {
  let mut session = Session::new();
  let invocation = ToolInvocation {
    id: agent_core::ToolCallId::new(),
    name: "read_file".into(),
    arguments: json!({"path": "README.md"}),
  };
  let mut assistant = message(Role::Assistant, "");
  assistant.tool_calls.push(invocation.clone());
  assistant.tool_calls.push(ToolInvocation {
    id: agent_core::ToolCallId::new(),
    name: "read_file".into(),
    arguments: json!({"path": "Cargo.toml"}),
  });
  session.push_unbudgeted_message(assistant.clone());
  let mut observation = message(Role::Tool, "contents");
  observation.tool_call_id = Some(invocation.id);
  observation.tool_name = Some(invocation.name.clone());
  let observation_id = session.push_tool_observation(observation, &assistant);

  assert!(session.selected_messages(&[]).is_empty());
  let selected = session.selected_messages(&[observation_id]);
  assert_eq!(selected.len(), 2);
  assert_eq!(selected[0].role, Role::Assistant);
  assert_eq!(selected[0].tool_calls, [invocation]);
  assert_eq!(selected[1].role, Role::Tool);
}

#[test]
fn replay_requires_a_session_identity() {
  let events: Vec<AgentEvent> = Vec::new();
  assert!(Session::from_events(&events).is_err());
}

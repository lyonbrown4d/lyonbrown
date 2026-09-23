use std::collections::{HashMap, HashSet};

use agent_core::{
  AgentError, AgentEvent, ContextItem, ContextItemKind, DataClassification, SessionId, StepId,
  StopReason, ToolCallId, ToolInvocation, ToolResult,
};
use agent_model::{ChatMessage, Role};

struct StoredMessage {
  message: ChatMessage,
  context_index: Option<usize>,
}

pub struct Session {
  pub id: SessionId,
  pub working_memory: Vec<ContextItem>,
  messages: Vec<StoredMessage>,
  pub started: bool,
}

impl Session {
  pub fn new() -> Self {
    Self {
      id: SessionId::new(),
      working_memory: Vec::new(),
      messages: Vec::new(),
      started: false,
    }
  }

  pub(crate) fn from_events(events: &[AgentEvent]) -> Result<Self, AgentError> {
    let id = events
      .iter()
      .find_map(|event| match event {
        AgentEvent::SessionStarted { session_id, .. }
        | AgentEvent::UserMessageReceived { session_id, .. } => Some(*session_id),
        _ => None,
      })
      .ok_or_else(|| AgentError::Storage("event log has no session ID".into()))?;
    let mut session = Self::new();
    session.id = id;
    session.started = events
      .iter()
      .any(|event| matches!(event, AgentEvent::SessionStarted { .. }));
    let mut invocations: HashMap<StepId, Vec<ToolInvocation>> = HashMap::new();
    let mut results: HashMap<ToolCallId, ToolResult> = HashMap::new();
    let mut started_calls = HashSet::new();
    let mut pending_order = Vec::new();
    let mut step_text: HashMap<StepId, String> = HashMap::new();
    for event in events {
      match event {
        AgentEvent::ModelDelta { step_id, delta } => {
          step_text.entry(*step_id).or_default().push_str(delta);
        }
        AgentEvent::UserMessageReceived {
          session_id, text, ..
        } => {
          if *session_id != id {
            return Err(AgentError::Storage("event log mixes session IDs".into()));
          }
          replay_pending_tools(
            &mut session,
            &mut invocations,
            &mut results,
            &started_calls,
            &mut pending_order,
            &mut step_text,
          );
          session.push_message(
            ChatMessage {
              role: Role::User,
              content: text.clone(),
              tool_call_id: None,
              tool_name: None,
              tool_calls: Vec::new(),
            },
            ContextItemKind::Message,
            "user",
          );
        }
        AgentEvent::ToolRequested {
          step_id,
          invocation,
          ..
        } => {
          if !invocations.contains_key(step_id) {
            pending_order.push(*step_id);
          }
          invocations
            .entry(*step_id)
            .or_default()
            .push(invocation.clone());
        }
        AgentEvent::ToolStarted { tool_call_id, .. } => {
          started_calls.insert(*tool_call_id);
        }
        AgentEvent::ToolCompleted { result, .. } => {
          results.insert(result.invocation_id, result.clone());
        }
        AgentEvent::StepCompleted {
          step_id, outcome, ..
        } if matches!(outcome.stop_reason, StopReason::ToolUse) => {
          let mut calls = invocations.remove(step_id).unwrap_or_default();
          pending_order.retain(|pending| pending != step_id);
          step_text.remove(step_id);
          calls.retain(|call| results.contains_key(&call.id));
          let assistant = ChatMessage {
            role: Role::Assistant,
            content: outcome.text.clone(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: calls.clone(),
          };
          session.push_unbudgeted_message(assistant.clone());
          for call in calls {
            let result = results
              .remove(&call.id)
              .expect("retained calls have results");
            let content = if result.ok {
              result.output
            } else {
              serde_json::json!({
                "ok": false,
                "output": result.output,
                "error": result.error,
              })
              .to_string()
            };
            session.push_tool_observation(
              ChatMessage {
                role: Role::Tool,
                content,
                tool_call_id: Some(call.id),
                tool_name: Some(call.name),
                tool_calls: Vec::new(),
              },
              &assistant,
            );
          }
        }
        AgentEvent::TurnCompleted { answer, .. } => {
          session.push_message(
            ChatMessage {
              role: Role::Assistant,
              content: answer.clone(),
              tool_call_id: None,
              tool_name: None,
              tool_calls: Vec::new(),
            },
            ContextItemKind::Message,
            "assistant",
          );
        }
        _ => {}
      }
    }
    replay_pending_tools(
      &mut session,
      &mut invocations,
      &mut results,
      &started_calls,
      &mut pending_order,
      &mut step_text,
    );
    Ok(session)
  }
}

fn replay_pending_tools(
  session: &mut Session,
  invocations: &mut HashMap<StepId, Vec<ToolInvocation>>,
  results: &mut HashMap<ToolCallId, ToolResult>,
  started_calls: &HashSet<ToolCallId>,
  pending_order: &mut Vec<StepId>,
  step_text: &mut HashMap<StepId, String>,
) {
  for step_id in pending_order.drain(..) {
    let Some(calls) = invocations.remove(&step_id) else {
      continue;
    };
    let assistant = ChatMessage {
      role: Role::Assistant,
      content: step_text.remove(&step_id).unwrap_or_default(),
      tool_call_id: None,
      tool_name: None,
      tool_calls: calls.clone(),
    };
    session.push_unbudgeted_message(assistant.clone());
    for call in calls {
      let content = match results.remove(&call.id) {
        Some(result) if result.ok => result.output,
        Some(result) => serde_json::json!({
          "ok": false,
          "output": result.output,
          "error": result.error,
        })
        .to_string(),
        None if started_calls.contains(&call.id) => {
          "Tool invocation was interrupted; outcome unknown. Do not repeat automatically.".into()
        }
        None => "Tool invocation was not started before interruption.".into(),
      };
      session.push_tool_observation(
        ChatMessage {
          role: Role::Tool,
          content,
          tool_call_id: Some(call.id),
          tool_name: Some(call.name),
          tool_calls: Vec::new(),
        },
        &assistant,
      );
    }
  }
}

#[path = "session/messages.rs"]
mod messages;

impl Default for Session {
  fn default() -> Self {
    Self::new()
  }
}

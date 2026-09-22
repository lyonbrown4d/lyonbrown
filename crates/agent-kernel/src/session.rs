use std::collections::HashSet;

use agent_core::{ContextItem, ContextItemKind, DataClassification, SessionId};
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

    pub(crate) fn push_message(
        &mut self,
        message: ChatMessage,
        kind: ContextItemKind,
        source: impl Into<String>,
    ) -> ContextItem {
        let serialized =
            serde_json::to_string(&message).expect("ChatMessage serialization cannot fail");
        let mut item = ContextItem::new(kind, serialized, 0);
        item.source = source.into();
        let context_index = self.working_memory.len();
        self.working_memory.push(item.clone());
        self.messages.push(StoredMessage {
            message,
            context_index: Some(context_index),
        });
        item
    }

    pub(crate) fn push_unbudgeted_message(&mut self, message: ChatMessage) {
        self.messages.push(StoredMessage {
            message,
            context_index: None,
        });
    }

    pub(crate) fn push_tool_observation(
        &mut self,
        message: ChatMessage,
        assistant: &ChatMessage,
    ) -> ContextItem {
        let serialized = serde_json::to_string(&(assistant, &message))
            .expect("ChatMessage serialization cannot fail");
        let mut item = ContextItem::new(ContextItemKind::ToolObservation, serialized, 0);
        item.source = format!(
            "tool://{}#{}",
            message.tool_name.as_deref().unwrap_or("unknown"),
            message
                .tool_call_id
                .map_or_else(|| "unknown".into(), |id| id.to_string())
        );
        let context_index = self.working_memory.len();
        self.working_memory.push(item.clone());
        self.messages.push(StoredMessage {
            message,
            context_index: Some(context_index),
        });
        item
    }

    pub(crate) fn selected_messages(&self, selected: &[ContextItem]) -> Vec<ChatMessage> {
        let selected_ids: HashSet<_> = selected.iter().map(|item| item.id).collect();
        let mapped_ids: HashSet<_> = self
            .messages
            .iter()
            .filter_map(|stored| stored.context_index)
            .map(|index| self.working_memory[index].id)
            .collect();
        let selected_tool_calls: HashSet<_> = self
            .messages
            .iter()
            .filter(|stored| {
                stored
                    .context_index
                    .is_some_and(|index| selected_ids.contains(&self.working_memory[index].id))
                    && stored.message.role == Role::Tool
            })
            .filter_map(|stored| stored.message.tool_call_id)
            .collect();
        let mut messages = selected
            .iter()
            .filter(|item| {
                !mapped_ids.contains(&item.id) && item.classification != DataClassification::Secret
            })
            .filter_map(|selected_item| {
                self.working_memory
                    .iter()
                    .find(|item| item.id == selected_item.id)
            })
            .map(synthetic_context_message)
            .collect::<Vec<_>>();
        messages.extend(self.messages.iter().filter_map(|stored| {
            if stored
                .context_index
                .is_some_and(|index| selected_ids.contains(&self.working_memory[index].id))
            {
                return Some(stored.message.clone());
            }
            let mut assistant = stored.message.clone();
            assistant
                .tool_calls
                .retain(|call| selected_tool_calls.contains(&call.id));
            (!assistant.tool_calls.is_empty()).then_some(assistant)
        }));
        messages
    }

    pub(crate) fn budget_items(&self) -> Vec<ContextItem> {
        let mapped_indices: HashSet<_> = self
            .messages
            .iter()
            .filter_map(|stored| stored.context_index)
            .collect();
        self.working_memory
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut budget_item = item.clone();
                if !mapped_indices.contains(&index)
                    && item.classification != DataClassification::Secret
                {
                    budget_item.content = serde_json::to_string(&synthetic_context_message(item))
                        .expect("ChatMessage serialization cannot fail");
                }
                budget_item
            })
            .collect()
    }
}

fn synthetic_context_message(item: &ContextItem) -> ChatMessage {
    let source = if item.source.is_empty() {
        "unknown"
    } else {
        &item.source
    };
    ChatMessage {
        role: Role::System,
        content: format!("[Context {:?} from {source}]\n{}", item.kind, item.content),
        tool_call_id: None,
        tool_name: None,
        tool_calls: Vec::new(),
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    use agent_core::{ContextItemKind, ToolInvocation};
    use agent_model::{ChatMessage, Role};
    use serde_json::json;

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
}

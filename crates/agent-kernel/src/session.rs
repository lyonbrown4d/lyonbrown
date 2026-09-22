use agent_core::{ContextItem, ContextItemKind, SessionId};
use agent_model::ChatMessage;

pub struct Session {
    pub id: SessionId,
    pub working_memory: Vec<ContextItem>,
    pub messages: Vec<ChatMessage>,
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

    pub(crate) fn remember(
        &mut self,
        kind: ContextItemKind,
        content: impl Into<String>,
        source: impl Into<String>,
    ) {
        let mut item = ContextItem::new(kind, content, 0);
        item.source = source.into();
        self.working_memory.push(item);
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

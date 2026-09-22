use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use agent_core::ToolDefinition;

use crate::Tool;

#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: Arc<RwLock<HashMap<String, Arc<dyn Tool>>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.tools
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(tool.definition().name.clone(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions: Vec<_> = self
            .tools
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .map(|tool| tool.definition().clone())
            .collect();
        definitions.sort_by(|left, right| left.name.cmp(&right.name));
        definitions
    }
}

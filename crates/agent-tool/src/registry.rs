use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use agent_core::{ToolDefinition, ToolInvocation};

use crate::Tool;

#[derive(Clone, Default)]
pub struct ToolRegistry {
  tools: Arc<RwLock<HashMap<String, RegisteredTool>>>,
}

struct RegisteredTool {
  tool: Arc<dyn Tool>,
  validator: Result<Arc<jsonschema::Validator>, Arc<str>>,
}

impl ToolRegistry {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn register(&self, tool: Arc<dyn Tool>) {
    let validator = jsonschema::validator_for(&tool.definition().input_schema)
      .map(Arc::new)
      .map_err(|error| Arc::<str>::from(error.to_string()));
    self
      .tools
      .write()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .insert(
        tool.definition().name.clone(),
        RegisteredTool { tool, validator },
      );
  }

  pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
    self
      .tools
      .read()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .get(name)
      .map(|registered| registered.tool.clone())
  }

  pub(crate) fn validate_arguments(
    &self,
    invocation: &ToolInvocation,
  ) -> Result<(), SchemaValidationError> {
    let tools = self
      .tools
      .read()
      .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(registered) = tools.get(&invocation.name) else {
      return Ok(());
    };
    let validator = registered
      .validator
      .as_ref()
      .map_err(|error| SchemaValidationError::InvalidSchema(error.to_string()))?;
    if let Some(error) = validator.iter_errors(&invocation.arguments).next() {
      return Err(SchemaValidationError::InvalidArguments(error.to_string()));
    }
    Ok(())
  }

  pub fn definitions(&self) -> Vec<ToolDefinition> {
    let mut definitions: Vec<_> = self
      .tools
      .read()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .values()
      .map(|registered| registered.tool.definition().clone())
      .collect();
    definitions.sort_by(|left, right| left.name.cmp(&right.name));
    definitions
  }
}

pub(crate) enum SchemaValidationError {
  InvalidSchema(String),
  InvalidArguments(String),
}

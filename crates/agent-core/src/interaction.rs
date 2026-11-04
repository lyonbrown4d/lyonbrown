//! Product execution modes only restrict authority granted by daemon policy.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InteractionMode {
  ReadOnly,
  Plan,
  #[default]
  Execute,
}
impl InteractionMode {
  pub fn permits(self, tool: &str) -> bool {
    match self {
      Self::Execute => true,
      Self::Plan if tool == "plan_update" => true,
      Self::Plan | Self::ReadOnly => matches!(
        tool,
        "read_file"
          | "list_files"
          | "search_files"
          | "plan_read"
          | "checkpoint_read"
          | "memory_read"
          | "memory_search"
          | "load_skill"
      ),
    }
  }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct InteractionPolicy {
  pub revision: u64,
  pub mode: InteractionMode,
}

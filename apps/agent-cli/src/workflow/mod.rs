//! Pure presentation and operation intents; daemon ownership and authorization stay remote.
mod approval;
mod render;
mod selection;
mod task;

pub use approval::ApprovalSelection;
pub use render::{render_approvals, render_tasks};
pub use task::{TaskIntent, TaskOperation, TaskSelection};

#[derive(Debug, Default)]
pub struct WorkflowState {
  pub approvals: ApprovalSelection,
  pub tasks: TaskSelection,
}

/// Render controls visibly so neither terminal escapes nor bidi overrides can hide arguments.
/// The original protocol payload remains unchanged in the selection state.
pub fn safe_text(value: &str) -> String {
  let mut output = String::new();
  for character in value.chars() {
    if (character.is_control() && character != '\n' && character != '\t')
      || matches!(character, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    {
      output.extend(character.escape_unicode());
    } else {
      output.push(character);
    }
  }
  output
}

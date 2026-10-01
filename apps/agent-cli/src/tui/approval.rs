use agent_app::ApprovalView;
use agent_core::{ToolCallId, ToolPreview};

#[derive(Debug, Clone)]
pub struct ApprovalRequest {
  pub request_id: ToolCallId,
  pub preview: ToolPreview,
}

impl ApprovalRequest {
  pub(crate) fn from_view(view: &ApprovalView) -> Option<Self> {
    Some(Self {
      request_id: view.request_id,
      preview: view.preview.clone()?,
    })
  }
}

pub(crate) fn approval_safe(value: &str) -> String {
  value
    .chars()
    .map(|character| match character {
      '\n' => "\\n".to_owned(),
      '\r' => "\\r".to_owned(),
      '\t' => "\\t".to_owned(),
      '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => {
        "�".to_owned()
      }
      _ if character.is_control() => "�".to_owned(),
      _ => character.to_string(),
    })
    .collect()
}

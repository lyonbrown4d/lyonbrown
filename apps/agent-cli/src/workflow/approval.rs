use agent_protocol::v1;
use ratatui::text::{Line, Text};
use serde_json::Value;

use super::{safe_text, selection::Selection};

#[derive(Debug, Default)]
pub struct ApprovalSelection {
  rows: Vec<v1::Approval>,
  selection: Selection,
  expanded: bool,
}

impl ApprovalSelection {
  pub fn replace(&mut self, approvals: Vec<v1::Approval>) {
    let ids = approvals
      .iter()
      .map(|row| row.id.as_str())
      .collect::<Vec<_>>();
    self.selection.replace(self.rows.is_empty(), &ids);
    self.rows = approvals;
  }

  pub fn rows(&self) -> &[v1::Approval] {
    &self.rows
  }

  pub fn selected_id(&self) -> Option<&str> {
    self.selection.id.as_deref()
  }

  pub fn selected(&self) -> Option<&v1::Approval> {
    self
      .rows
      .iter()
      .find(|row| Some(row.id.as_str()) == self.selected_id())
  }

  pub fn next(&mut self) {
    self.step(true);
  }

  pub fn previous(&mut self) {
    self.step(false);
  }

  fn step(&mut self, forward: bool) {
    let ids = self
      .rows
      .iter()
      .map(|row| row.id.as_str())
      .collect::<Vec<_>>();
    self.selection.step(&ids, forward);
    self.expanded = false;
  }

  /// Both approve and reject act on exactly this ID; never on the whole queue.
  pub fn decision_id(&self, now_ms: i64) -> Option<&str> {
    self
      .selected()
      .filter(|row| row.status == "pending" && row.expires_at_ms > now_ms)
      .map(|row| row.id.as_str())
  }

  pub fn toggle_expanded(&mut self) {
    self.expanded = !self.expanded;
  }

  pub fn detail(&self) -> Text<'static> {
    let Some(row) = self.selected() else {
      return Text::raw("Select an approval with ↑/↓. No request is selected.");
    };
    let mut lines = vec![
      Line::raw(format!("Approval: {}", safe_text(&row.id))),
      Line::raw(format!(
        "Tool: {} · {}",
        safe_text(&row.tool),
        safe_text(&row.status)
      )),
      Line::raw(format!(
        "Run: {} · expires at {} ms",
        safe_text(&row.run_id),
        row.expires_at_ms
      )),
    ];
    if let Ok(Value::Object(arguments)) = serde_json::from_str::<Value>(&row.arguments_json) {
      for key in [
        "path",
        "file_path",
        "target",
        "command",
        "executable",
        "cwd",
        "expected_version",
        "version",
        "base_hash",
      ] {
        if let Some(value) = arguments.get(key) {
          lines.push(Line::raw(format!(
            "{key}: {}",
            safe_text(&value.to_string())
          )));
        }
      }
      if arguments.contains_key("content") || arguments.contains_key("new_content") {
        lines.push(Line::raw(
          "Write content is included in the exact arguments below.",
        ));
      }
    }
    lines.push(Line::raw(
      "Exact arguments (controls escaped for safe display):",
    ));
    let display = safe_text(&row.arguments_json);
    let total = display.lines().count();
    for (index, line) in display.lines().enumerate() {
      if !self.expanded && index >= 12 {
        break;
      }
      let shown: String = if self.expanded {
        line.to_owned()
      } else {
        line.chars().take(240).collect()
      };
      let suffix = if !self.expanded && line.chars().count() > 240 {
        " … [expand for full line]"
      } else {
        ""
      };
      lines.push(Line::raw(format!("{shown}{suffix}")));
    }
    if !self.expanded && total > 12 {
      lines.push(Line::raw(format!(
        "… {} more lines; expand to inspect",
        total - 12
      )));
    }
    lines.push(Line::raw(
      "Ctrl+Y approve · Ctrl+E reject · Space expand/collapse · ↑/↓ select",
    ));
    lines.push(Line::raw(
      "Decision covers the complete request. Partial diff approval is unavailable.",
    ));
    Text::from(lines)
  }
}

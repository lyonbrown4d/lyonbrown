use ratatui::text::{Line, Text};
use serde_json::Value;

use super::{safe_text, selection::Selection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOperation {
  Inspect,
  Reconcile,
  Cancel,
  Accept,
  Reject,
}

/// Intent contains identity only; agentd resolves and authorizes the original Worker route.
/// The caller must resolve and authenticate it; an intent does not confer permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIntent {
  pub task_id: String,
  pub operation: TaskOperation,
}

/// Accepts the task_management JSON contract: envelope.task_id, status,
/// attempt_status, result and error. No Worker directory is present in that contract.
#[derive(Debug, Default)]
pub struct TaskSelection {
  rows: Vec<Value>,
  selection: Selection,
}

pub(super) fn task_id(row: &Value) -> Option<&str> {
  row
    .get("envelope")?
    .get("task_id")?
    .as_str()
    .filter(|id| !id.is_empty())
}

impl TaskSelection {
  pub fn replace(&mut self, rows: Vec<Value>) {
    let rows = rows
      .into_iter()
      .filter(|row| task_id(row).is_some())
      .collect::<Vec<_>>();
    let ids = rows.iter().filter_map(task_id).collect::<Vec<_>>();
    self.selection.replace(self.rows.is_empty(), &ids);
    self.rows = rows;
  }

  pub fn rows(&self) -> &[Value] {
    &self.rows
  }

  pub fn selected_id(&self) -> Option<&str> {
    self.selection.id.as_deref()
  }

  pub fn selected(&self) -> Option<&Value> {
    self
      .rows
      .iter()
      .find(|row| task_id(row) == self.selected_id())
  }

  pub fn next(&mut self) {
    self.step(true);
  }

  pub fn previous(&mut self) {
    self.step(false);
  }

  fn step(&mut self, forward: bool) {
    let ids = self.rows.iter().filter_map(task_id).collect::<Vec<_>>();
    self.selection.step(&ids, forward);
  }

  pub fn available_operations(&self) -> Vec<TaskOperation> {
    let Some(row) = self.selected() else {
      return Vec::new();
    };
    let mut operations = vec![TaskOperation::Inspect, TaskOperation::Reconcile];
    let status = row["status"].as_str().unwrap_or("");
    if matches!(
      status,
      "accepted" | "running" | "uncertain" | "awaiting_acceptance"
    ) {
      operations.push(TaskOperation::Cancel);
    }
    if status == "awaiting_acceptance" && row["attempt_status"] == "succeeded" {
      operations.extend([TaskOperation::Accept, TaskOperation::Reject]);
    }
    operations
  }

  pub fn intent(&self, operation: TaskOperation) -> Result<TaskIntent, &'static str> {
    let task_id = self.selected_id().ok_or("Select a Task first")?;
    if !self.available_operations().contains(&operation) {
      return Err("Operation unavailable in the current Task state; inspect or reconcile first");
    }
    Ok(TaskIntent {
      task_id: task_id.to_owned(),
      operation,
    })
  }

  pub fn detail(&self) -> Text<'static> {
    let Some(row) = self.selected() else {
      return Text::raw("Select a Task with ↑/↓.");
    };
    let mut lines = vec![
      Line::raw(format!(
        "Task: {}",
        safe_text(self.selected_id().unwrap_or(""))
      )),
      Line::raw(format!(
        "Status: {} · Attempt: {}",
        safe_text(row["status"].as_str().unwrap_or("unknown")),
        safe_text(row["attempt_status"].as_str().unwrap_or("unknown"))
      )),
      Line::raw("Worker Attempt success does not mean the Master has accepted the Task."),
      Line::raw(format!("Available: {:?}", self.available_operations())),
      Line::raw("i inspect · r reconcile · c cancel · a accept · x reject"),
      Line::raw("agentd resolves the original paired Worker route and rechecks authority."),
      Line::raw("Legacy Tasks without a stored origin require the explicit-target command."),
    ];
    for key in ["result", "error"] {
      if let Some(value) = row[key].as_str().filter(|value| !value.is_empty()) {
        lines.push(Line::raw(format!("{key} (untrusted Worker content):")));
        lines.extend(
          safe_text(value)
            .lines()
            .take(12)
            .map(|line| Line::raw(line.chars().take(240).collect::<String>())),
        );
      }
    }
    Text::from(lines)
  }
}

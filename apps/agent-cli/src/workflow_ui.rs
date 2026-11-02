use crate::model::App;

pub(crate) fn sync_task_index(app: &mut App) {
  if let Some(id) = app.workflow.tasks.selected_id() {
    app.task_selected = app
      .task_rows
      .iter()
      .position(|row| {
        row
          .pointer("/envelope/task_id")
          .and_then(serde_json::Value::as_str)
          == Some(id)
      })
      .unwrap_or(0);
  }
}

pub(crate) fn request_task(
  app: &mut App,
  actions: &tokio::sync::mpsc::Sender<crate::backend::Action>,
  key: char,
) -> anyhow::Result<()> {
  use crate::workflow::TaskOperation;
  let operation = match key {
    'r' => TaskOperation::Reconcile,
    'c' => TaskOperation::Cancel,
    'a' => TaskOperation::Accept,
    'x' => TaskOperation::Reject,
    _ => TaskOperation::Inspect,
  };
  let intent = match app.workflow.tasks.intent(operation) {
    Ok(intent) => intent,
    Err(message) => {
      app.notice = message.into();
      return Ok(());
    }
  };
  crate::input::send(
    actions,
    crate::backend::Action::TaskOperation {
      task_id: intent.task_id,
      operation: intent.operation,
      generation: app.generation,
      session: app.selected_session().map(|session| session.id.clone()),
    },
  )?;
  app.notice = "正在由 agentd 使用 Task 原始 Worker 身份核对操作。".into();
  Ok(())
}

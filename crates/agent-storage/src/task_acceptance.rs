use crate::{event_tx, schema::tasks};
use agent_core::{Task, TaskEnvelope};
use anyhow::Result;
use sea_orm::sea_query::ExprTrait;
use sea_orm::{QuerySelect, Set, prelude::*};
pub(crate) async fn accept_task_tx(
  connection: &impl ConnectionTrait,
  envelope: TaskEnvelope,
  worker_node_id: Option<&str>,
  worker_endpoint: Option<&str>,
) -> Result<(Task, bool)> {
  anyhow::ensure!(
    !envelope.task_id.is_empty()
      && !envelope.attempt_id.is_empty()
      && !envelope.idempotency_key.is_empty(),
    "task identity is required"
  );
  anyhow::ensure!(envelope.owner_epoch > 0, "invalid owner epoch");
  let existing: Option<String> = tasks::Entity::find()
    .filter(
      tasks::Column::Id
        .eq(&envelope.task_id)
        .or(tasks::Column::IdempotencyKey.eq(&envelope.idempotency_key)),
    )
    .select_only()
    .column(tasks::Column::Record)
    .into_tuple::<String>()
    .one(connection)
    .await?;
  if let Some(json) = existing {
    let value: Task = serde_json::from_str(&json)?;
    anyhow::ensure!(value.envelope == envelope, "conflicting task resubmission");
    return Ok((value, false));
  }
  let mut task = Task {
    envelope,
    status: "accepted".into(),
    attempt_status: "accepted".into(),
    result: String::new(),
    error: String::new(),
    cursor: 0,
  };
  let event = event_tx(
    connection,
    &task.envelope.task_id,
    "task_accepted",
    serde_json::json!({"attempt_id":task.envelope.attempt_id,"worker_node_id":worker_node_id,"worker_endpoint":worker_endpoint}),
  ).await?;
  task.cursor = event.cursor;
  tasks::ActiveModel {
    session_id: Set(task.envelope.session_id.clone()),
    owner_node_id: Set(task.envelope.owner_node_id.clone()),
    status: Set(task.status.clone()),
    attempt_status: Set(task.attempt_status.clone()),
    id: Set((&task.envelope.task_id).to_owned()),
    idempotency_key: Set((&task.envelope.idempotency_key).to_owned()),
    owner_epoch: Set((i64::try_from(task.envelope.owner_epoch)?).to_owned()),
    record: Set((serde_json::to_string(&task)?).to_owned()),
  }
  .insert(connection)
  .await?;
  Ok((task, true))
}

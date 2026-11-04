use super::entities::{plans, requests};
use crate::{Store, event_tx, schema::runs, verify_epoch};
use agent_planning::{Actor, Plan, PlanContext, PlanError, PlanMutation, VerificationRecord};
use anyhow::{Context, Result, ensure};
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use sha2::{Digest, Sha256};
use uuid::Uuid;

impl Store {
  pub async fn plan_apply(
    &self,
    context: &PlanContext,
    actor: Actor,
    mutation: &PlanMutation,
  ) -> Result<Plan> {
    if let agent_planning::PlanChange::VerifyStep {
      step_id,
      operation_id,
    } = &mutation.change
    {
      return self
        .plan_verify_operation(
          context,
          mutation.request_id,
          mutation.plan_id,
          mutation.expected_revision,
          *step_id,
          operation_id,
        )
        .await;
    }
    let digest = digest(&(context, actor, mutation))?;
    let transaction = self.begin_write().await?;
    let result = async {
      fence(&transaction, context).await?;
      if let Some(previous) = replay(&transaction, context, mutation.request_id, &digest).await? {
        return Ok(previous);
      }
      let current = load(&transaction, context, mutation.plan_id).await?;
      let plan = agent_planning::apply(current.as_ref(), context, actor, mutation)?;
      persist(
        &transaction,
        context,
        mutation.request_id,
        &digest,
        current.as_ref(),
        &plan,
        "plan_updated",
        serde_json::json!({"actor":actor,"change":mutation.change}),
      )
      .await?;
      Ok::<_, anyhow::Error>(plan)
    }
    .await;
    match result {
      Ok(plan) => {
        transaction.commit().await?;
        Ok(plan)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }

  /// Host-only execution receipt. Never expose this method as a model tool or client RPC.
  pub async fn plan_record_verification(
    &self,
    context: &PlanContext,
    request_id: Uuid,
    plan_id: Uuid,
    expected_revision: u64,
    record: VerificationRecord,
  ) -> Result<Plan> {
    ensure!(
      !request_id.is_nil(),
      PlanError::Invalid("nil request UUID".into())
    );
    let digest = digest(&(context, request_id, plan_id, expected_revision, &record))?;
    let transaction = self.begin_write().await?;
    let result = async {
      fence(&transaction, context).await?;
      if let Some(previous) = replay(&transaction, context, request_id, &digest).await? {
        return Ok(previous);
      }
      let previous = load(&transaction, context, plan_id)
        .await?
        .ok_or(PlanError::NotFound)?;
      let plan =
        agent_planning::record_verification(&previous, context, expected_revision, record)?;
      persist(
        &transaction,
        context,
        request_id,
        &digest,
        Some(&previous),
        &plan,
        "plan_verification_recorded",
        serde_json::json!({"verification":plan.verifications.last()}),
      )
      .await?;
      Ok::<_, anyhow::Error>(plan)
    }
    .await;
    match result {
      Ok(plan) => {
        transaction.commit().await?;
        Ok(plan)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
}
async fn fence(connection: &impl ConnectionTrait, context: &PlanContext) -> Result<()> {
  verify_epoch(connection, &context.session_id, context.owner_epoch).await?;
  let run = runs::Entity::find_by_id(&context.run_id)
    .one(connection)
    .await?
    .context("plan run missing")?;
  ensure!(run.session_id == context.session_id, PlanError::NotFound);
  Ok(())
}
async fn load(
  connection: &impl ConnectionTrait,
  context: &PlanContext,
  id: Uuid,
) -> Result<Option<Plan>> {
  plans::Entity::find_by_id(id.to_string())
    .filter(plans::Column::SessionId.eq(&context.session_id))
    .filter(plans::Column::RunId.eq(&context.run_id))
    .one(connection)
    .await?
    .map(|row| Ok(serde_json::from_str(&row.record)?))
    .transpose()
}
fn digest(value: &impl serde::Serialize) -> Result<String> {
  Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}
async fn replay(
  connection: &impl ConnectionTrait,
  context: &PlanContext,
  request: Uuid,
  digest: &str,
) -> Result<Option<Plan>> {
  if let Some(row) = requests::Entity::find_by_id((context.session_id.clone(), request.to_string()))
    .one(connection)
    .await?
  {
    ensure!(row.digest == digest, PlanError::RequestConflict);
    return Ok(Some(serde_json::from_str(&row.result)?));
  }
  Ok(None)
}
#[allow(clippy::too_many_arguments)]
async fn persist(
  connection: &impl ConnectionTrait,
  context: &PlanContext,
  request_id: Uuid,
  digest: &str,
  previous: Option<&Plan>,
  plan: &Plan,
  event: &str,
  detail: serde_json::Value,
) -> Result<()> {
  let record = serde_json::to_string(plan)?;
  if let Some(previous) = previous {
    let updated = plans::Entity::update_many()
      .filter(plans::Column::Id.eq(plan.id.to_string()))
      .filter(plans::Column::Revision.eq(i64::try_from(previous.revision)?))
      .col_expr(
        plans::Column::Revision,
        sea_orm::sea_query::Expr::value(i64::try_from(plan.revision)?),
      )
      .col_expr(
        plans::Column::Record,
        sea_orm::sea_query::Expr::value(record.clone()),
      )
      .exec(connection)
      .await?;
    ensure!(updated.rows_affected == 1, PlanError::Conflict);
  } else {
    plans::ActiveModel {
      id: Set(plan.id.to_string()),
      session_id: Set(context.session_id.clone()),
      run_id: Set(context.run_id.clone()),
      revision: Set(i64::try_from(plan.revision)?),
      record: Set(record.clone()),
    }
    .insert(connection)
    .await?;
  }
  requests::ActiveModel {
    session_id: Set(context.session_id.clone()),
    request_id: Set(request_id.to_string()),
    digest: Set(digest.into()),
    result: Set(record),
  }
  .insert(connection)
  .await?;
  event_tx(
    connection,
    &context.run_id,
    event,
    serde_json::json!({
      "plan_id":plan.id,"revision":plan.revision,"request_id":request_id,"detail":detail
    }),
  )
  .await?;
  Ok(())
}

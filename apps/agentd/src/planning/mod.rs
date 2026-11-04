//! Session-scoped plans record progress without granting tools, nodes, or models.
mod schema;
mod tools;
use crate::Daemon;
use agent_planning::{Actor, Plan, PlanContext, PlanMutation, PlanPage};
use anyhow::{Context, Result, ensure};
pub(crate) use tools::tools;
use uuid::Uuid;

/// The caller supplies an authenticated run binding, never an untrusted context object.
pub(crate) async fn context(daemon: &Daemon, run_id: &str) -> Result<PlanContext> {
  let run = daemon
    .store
    .run(run_id)
    .await?
    .context("plan run unavailable")?;
  let session = daemon
    .store
    .session(&run.session_id)
    .await?
    .context("plan session unavailable")?;
  ensure!(
    session.owner_node_id == daemon.node_id,
    "plan session has a different owner"
  );
  Ok(PlanContext {
    session_id: session.id,
    run_id: run.id,
    owner_epoch: session.owner_epoch,
  })
}

/// RPC adapters must authorize the run/session before calling this service.
pub(crate) async fn list(
  daemon: &Daemon,
  authenticated_run: &str,
  after: Option<Uuid>,
  limit: u32,
) -> Result<PlanPage> {
  let context = context(daemon, authenticated_run).await?;
  daemon
    .store
    .plan_list(&context.session_id, Some(&context.run_id), after, limit)
    .await
}
pub(crate) async fn get(
  daemon: &Daemon,
  authenticated_run: &str,
  id: Uuid,
) -> Result<Option<Plan>> {
  let context = context(daemon, authenticated_run).await?;
  Ok(
    daemon
      .store
      .plan_get(&context.session_id, id)
      .await?
      .filter(|plan| plan.run_id == context.run_id),
  )
}
/// This is the authenticated user's explicit edit boundary, including completion overrides.
pub(crate) async fn update_user(
  daemon: &Daemon,
  authenticated_run: &str,
  mutation: &PlanMutation,
) -> Result<Plan> {
  let context = context(daemon, authenticated_run).await?;
  daemon
    .store
    .plan_apply(&context, Actor::User, mutation)
    .await
}

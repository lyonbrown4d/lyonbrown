use super::{binding, convert, error, session};
use crate::Daemon;
use agent_planning::{PlanChange, PlanMutation};
use agent_protocol::v1;
use tonic::Status;
pub(super) async fn list(
  daemon: &Daemon,
  input: v1::ListWorkflowPlansRequest,
) -> Result<v1::WorkflowPlans, Status> {
  session(daemon, &input.session_id).await?;
  if !input.run_id.is_empty() {
    binding(daemon, &input.session_id, &input.run_id).await?;
  }
  let after = if input.after_id.is_empty() {
    None
  } else {
    Some(convert::id(&input.after_id)?)
  };
  let page = daemon
    .store
    .plan_list(
      &input.session_id,
      (!input.run_id.is_empty()).then_some(input.run_id.as_str()),
      after,
      input.limit,
    )
    .await
    .map_err(error)?;
  Ok(v1::WorkflowPlans {
    plans: page
      .plans
      .into_iter()
      .map(|value| {
        let mut plan = convert::plan(value);
        plan.detail_included = false;
        plan.verifications.clear();
        for step in &mut plan.steps {
          step.reported_result.clear();
          step.verification_refs.clear();
          step.user_override = None;
          if let Some(definition) = &mut step.definition {
            definition.acceptance.clear();
            definition.dependencies.clear();
          }
        }
        plan
      })
      .collect(),
    next_after: page.next_after.map(|id| id.to_string()).unwrap_or_default(),
  })
}
pub(super) async fn get(
  daemon: &Daemon,
  input: v1::GetWorkflowPlanRequest,
) -> Result<v1::WorkflowPlan, Status> {
  session(daemon, &input.session_id).await?;
  daemon
    .store
    .plan_get(&input.session_id, convert::id(&input.plan_id)?)
    .await
    .map_err(error)?
    .map(convert::plan)
    .ok_or_else(|| Status::not_found("plan not found"))
}
pub(super) async fn mutate(
  daemon: &Daemon,
  input: v1::MutateWorkflowPlanRequest,
) -> Result<v1::WorkflowPlan, Status> {
  let (_, run) = binding(daemon, &input.session_id, &input.run_id).await?;
  if !daemon
    .store
    .interaction_policy(&run.id)
    .await
    .map_err(error)?
    .mode
    .permits("plan_update")
  {
    return Err(Status::permission_denied(
      "interaction mode forbids plan updates",
    ));
  }
  use v1::mutate_workflow_plan_request::Change;
  let change = match input
    .change
    .ok_or_else(|| Status::invalid_argument("plan change required"))?
  {
    Change::Create(value) => PlanChange::Create {
      draft: convert::draft(value)?,
    },
    Change::Revise(value) => PlanChange::Revise {
      draft: convert::draft(value)?,
    },
    Change::UpdateStep(value) => PlanChange::UpdateStep {
      step_id: convert::id(&value.step_id)?,
      state: convert::state(value.state)?,
      reported_result: value.reported_result,
      verification_refs: value
        .verification_refs
        .iter()
        .map(|id| convert::id(id))
        .collect::<Result<_, _>>()?,
      user_override: value.user_override,
    },
    Change::VerifyStep(value) => PlanChange::VerifyStep {
      step_id: convert::id(&value.step_id)?,
      operation_id: value.operation_id,
    },
  };
  let mutation = PlanMutation {
    request_id: convert::id(&input.request_id)?,
    plan_id: convert::id(&input.plan_id)?,
    expected_revision: input.expected_revision,
    change,
  };
  let plan = crate::planning::update_user(daemon, &run.id, &mutation)
    .await
    .map_err(error)?;
  daemon.events.notify_waiters();
  Ok(convert::plan(plan))
}

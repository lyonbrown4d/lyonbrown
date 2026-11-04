use super::{Reply, WorkflowPane};
use agent_protocol::v1;
use gpui_kit::*;
impl WorkflowPane {
  pub(super) fn create(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    let title = self.title.read(cx).value().to_string();
    let step = self.step_title.read(cx).value().to_string();
    let acceptance = self.acceptance.read(cx).value().to_string();
    if [title.as_str(), step.as_str(), acceptance.as_str()]
      .iter()
      .any(|value| value.trim().is_empty())
    {
      self.message = "Enter the plan title, first step and an acceptance criterion.".into();
      cx.notify();
      return;
    }
    self.request(
      true,
      async move {
        let run_id = client
          .list_runs(&session)
          .await?
          .last()
          .map(|run| run.id.clone())
          .ok_or_else(|| {
            anyhow::anyhow!("Create a task first to bind the plan to a persisted run")
          })?;
        Ok(Reply::Plan(
          client
            .mutate_plan(v1::MutateWorkflowPlanRequest {
              session_id: session,
              run_id,
              request_id: id(),
              plan_id: id(),
              expected_revision: 0,
              change: Some(v1::mutate_workflow_plan_request::Change::Create(
                v1::WorkflowPlanDraft {
                  title,
                  steps: vec![v1::WorkflowStepDraft {
                    id: id(),
                    title: step,
                    dependencies: vec![],
                    acceptance: vec![acceptance],
                  }],
                },
              )),
            })
            .await?,
        ))
      },
      cx,
    );
  }
  pub(super) fn update_step(
    &mut self,
    step_id: String,
    state: v1::WorkflowStepState,
    cx: &mut Context<Self>,
  ) {
    let (Some(client), Some(session), Some(plan)) =
      (self.client.clone(), self.session.clone(), self.plan.clone())
    else {
      return;
    };
    let report = self.report.read(cx).value().to_string();
    let receipt = plan
      .verifications
      .iter()
      .rev()
      .find(|record| record.step_id == step_id)
      .filter(|record| record.passed);
    let verified = receipt
      .map(|record| vec![record.id.clone()])
      .unwrap_or_default();
    let override_reason = if state == v1::WorkflowStepState::Completed && verified.is_empty() {
      if report.trim().is_empty() {
        self.message =
          "Completion needs host verification or your explicit explanation in the report field."
            .into();
        cx.notify();
        return;
      }
      Some(report.clone())
    } else {
      None
    };
    self.request(
      true,
      async move {
        Ok(Reply::Plan(
          client
            .mutate_plan(v1::MutateWorkflowPlanRequest {
              session_id: session,
              run_id: plan.run_id,
              request_id: id(),
              plan_id: plan.id,
              expected_revision: plan.revision,
              change: Some(v1::mutate_workflow_plan_request::Change::UpdateStep(
                v1::UpdateWorkflowStep {
                  step_id,
                  state: state as i32,
                  reported_result: report,
                  verification_refs: verified,
                  user_override: override_reason,
                },
              )),
            })
            .await?,
        ))
      },
      cx,
    );
  }
  pub(super) fn verify(&mut self, step_id: String, cx: &mut Context<Self>) {
    let (Some(client), Some(session), Some(plan)) =
      (self.client.clone(), self.session.clone(), self.plan.clone())
    else {
      return;
    };
    let operation = self.operation.read(cx).value().to_string();
    if operation.is_empty() {
      self.message = "Enter the real host command operation ID first.".into();
      cx.notify();
      return;
    }
    self.request(
      true,
      async move {
        Ok(Reply::Plan(
          client
            .mutate_plan(v1::MutateWorkflowPlanRequest {
              session_id: session,
              run_id: plan.run_id,
              request_id: id(),
              plan_id: plan.id,
              expected_revision: plan.revision,
              change: Some(v1::mutate_workflow_plan_request::Change::VerifyStep(
                v1::VerifyWorkflowStep {
                  step_id,
                  operation_id: operation,
                },
              )),
            })
            .await?,
        ))
      },
      cx,
    );
  }
}
fn id() -> String {
  uuid::Uuid::now_v7().to_string()
}

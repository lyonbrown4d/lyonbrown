use agent_planning::{Plan, PlanDraft, StepDraft, StepState};
use agent_protocol::v1;
use tonic::Status;
use uuid::Uuid;
pub(super) fn id(value: &str) -> Result<Uuid, Status> {
  Uuid::parse_str(value)
    .ok()
    .filter(|id| !id.is_nil())
    .ok_or_else(|| Status::invalid_argument("non-nil UUID required"))
}
pub(super) fn draft(value: v1::WorkflowPlanDraft) -> Result<PlanDraft, Status> {
  Ok(PlanDraft {
    title: value.title,
    steps: value
      .steps
      .into_iter()
      .map(|step| {
        Ok(StepDraft {
          id: id(&step.id)?,
          title: step.title,
          dependencies: step
            .dependencies
            .iter()
            .map(|dependency| id(dependency))
            .collect::<Result<_, _>>()?,
          acceptance: step.acceptance,
        })
      })
      .collect::<Result<_, Status>>()?,
  })
}
pub(super) fn state(value: i32) -> Result<StepState, Status> {
  match v1::WorkflowStepState::try_from(value) {
    Ok(v1::WorkflowStepState::Pending) => Ok(StepState::Pending),
    Ok(v1::WorkflowStepState::Running) => Ok(StepState::Running),
    Ok(v1::WorkflowStepState::Blocked) => Ok(StepState::Blocked),
    Ok(v1::WorkflowStepState::Completed) => Ok(StepState::Completed),
    Ok(v1::WorkflowStepState::Skipped) => Ok(StepState::Skipped),
    _ => Err(Status::invalid_argument("step state required")),
  }
}
pub(super) fn plan(value: Plan) -> v1::WorkflowPlan {
  v1::WorkflowPlan {
    detail_included: true,
    id: value.id.to_string(),
    session_id: value.session_id,
    run_id: value.run_id,
    owner_epoch: value.owner_epoch,
    revision: value.revision,
    title: value.title,
    steps: value
      .steps
      .into_iter()
      .map(|step| v1::WorkflowStep {
        definition: Some(v1::WorkflowStepDraft {
          id: step.definition.id.to_string(),
          title: step.definition.title,
          dependencies: step
            .definition
            .dependencies
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
          acceptance: step.definition.acceptance,
        }),
        state: match step.state {
          StepState::Pending => v1::WorkflowStepState::Pending,
          StepState::Running => v1::WorkflowStepState::Running,
          StepState::Blocked => v1::WorkflowStepState::Blocked,
          StepState::Completed => v1::WorkflowStepState::Completed,
          StepState::Skipped => v1::WorkflowStepState::Skipped,
        } as i32,
        reported_result: step.reported_result,
        verification_refs: step
          .verification_refs
          .into_iter()
          .map(|id| id.to_string())
          .collect(),
        user_override: step.user_override,
      })
      .collect(),
    verifications: value
      .verifications
      .into_iter()
      .map(|record| v1::WorkflowVerification {
        id: record.id.to_string(),
        step_id: record.step_id.to_string(),
        evidence_ref: record.evidence_ref,
        acceptance: record.acceptance,
        passed: record.passed,
        summary: record.summary,
      })
      .collect(),
  }
}
pub(super) fn change(
  value: agent_changes::ChangeSet,
  validations: Vec<agent_changes::ValidationRecord>,
) -> v1::WorkflowChange {
  use agent_changes::ChangeState;
  v1::WorkflowChange {
    detail_included: true,
    id: value.id,
    run_id: value.run_id,
    owner_epoch: value.owner_epoch,
    revision: value.revision,
    state: match value.state {
      ChangeState::Preview => v1::WorkflowChangeState::Preview,
      ChangeState::Prepared => v1::WorkflowChangeState::Prepared,
      ChangeState::Applied => v1::WorkflowChangeState::Applied,
      ChangeState::Uncertain => v1::WorkflowChangeState::Uncertain,
    } as i32,
    preview_sha256: value.preview_sha256,
    reverses: value.reverses,
    action_id: value.action_id,
    written_paths: value.written_paths,
    files: value
      .files
      .into_iter()
      .map(|file| v1::WorkflowChangeFile {
        path: file.path,
        expected_revision: file.expected_revision,
        before: file.before,
        after: file.after,
        before_sha256: file.before_sha256,
        after_sha256: file.after_sha256,
        unified_diff: file.unified_diff,
      })
      .collect(),
    validations: validations
      .into_iter()
      .map(|record| v1::WorkflowChangeValidation {
        id: record.id,
        change_id: record.change_id,
        change_revision: record.change_revision,
        change_sha256: record.change_sha256,
        operation_id: record.operation_id,
        passed: record.passed,
        verifies_current_workspace: record.verifies_current_workspace,
        exit_code: record.exit_code,
      })
      .collect(),
  }
}

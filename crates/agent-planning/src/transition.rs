use crate::{
  Actor, Plan, PlanChange, PlanContext, PlanError, PlanMutation, PlanStep, Result, StepState,
  VerificationRecord, validate_draft, validation::text,
};
use std::collections::BTreeSet;

pub fn apply(
  current: Option<&Plan>,
  context: &PlanContext,
  actor: Actor,
  mutation: &PlanMutation,
) -> Result<Plan> {
  if mutation.request_id.is_nil() || mutation.plan_id.is_nil() {
    return Err(PlanError::Invalid("nil request or plan UUID".into()));
  }
  let mut plan = match (&mutation.change, current) {
    (PlanChange::Create { draft }, None) if mutation.expected_revision == 0 => {
      validate_draft(draft)?;
      Plan {
        id: mutation.plan_id,
        session_id: context.session_id.clone(),
        run_id: context.run_id.clone(),
        owner_epoch: context.owner_epoch,
        revision: 0,
        title: draft.title.clone(),
        steps: draft.steps.iter().cloned().map(pending).collect(),
        verifications: Vec::new(),
      }
    }
    (PlanChange::Create { .. }, _) => return Err(PlanError::Conflict),
    (_, Some(plan)) if plan.revision == mutation.expected_revision => plan.clone(),
    (_, Some(_)) => return Err(PlanError::Conflict),
    (_, None) => return Err(PlanError::NotFound),
  };
  if plan.id != mutation.plan_id
    || plan.session_id != context.session_id
    || plan.run_id != context.run_id
  {
    return Err(PlanError::NotFound);
  }
  match &mutation.change {
    PlanChange::Create { .. } => (),
    PlanChange::VerifyStep { .. } => return Err(PlanError::Forbidden),
    PlanChange::Revise { draft } => {
      validate_draft(draft)?;
      for previous in &plan.steps {
        if previous.state != StepState::Pending
          && !draft.steps.iter().any(|step| step == &previous.definition)
        {
          return Err(PlanError::Invalid(
            "only pending steps may be redefined or removed".into(),
          ));
        }
      }
      plan.steps = draft
        .steps
        .iter()
        .map(|definition| {
          plan
            .steps
            .iter()
            .find(|step| &step.definition == definition)
            .cloned()
            .unwrap_or_else(|| pending(definition.clone()))
        })
        .collect();
      plan.title = draft.title.clone();
    }
    PlanChange::UpdateStep {
      step_id,
      state,
      reported_result,
      verification_refs,
      user_override,
    } => {
      if reported_result.len() > 16384 {
        return Err(PlanError::Invalid("oversized step report".into()));
      }
      let index = plan
        .steps
        .iter()
        .position(|step| step.definition.id == *step_id)
        .ok_or(PlanError::NotFound)?;
      let step = &plan.steps[index];
      let user_completion = actor == Actor::User
        && user_override.is_some()
        && *state == StepState::Completed
        && step.state == StepState::Pending;
      let allowed = step.state == *state
        || user_completion
        || matches!(
          (step.state, state),
          (StepState::Pending, StepState::Running | StepState::Skipped)
            | (
              StepState::Running,
              StepState::Blocked | StepState::Completed | StepState::Skipped
            )
            | (
              StepState::Blocked,
              StepState::Running | StepState::Completed | StepState::Skipped
            )
        );
      if !allowed {
        return Err(PlanError::Invalid("invalid step state transition".into()));
      }
      if matches!(state, StepState::Running | StepState::Completed)
        && step.definition.dependencies.iter().any(|dependency| {
          !plan
            .steps
            .iter()
            .any(|step| step.definition.id == *dependency && step.state == StepState::Completed)
        })
      {
        return Err(PlanError::Invalid("dependencies are not completed".into()));
      }
      if let Some(reason) = user_override {
        if actor != Actor::User || *state != StepState::Completed {
          return Err(PlanError::Forbidden);
        }
        text(reason, 2048)?;
      }
      let unique = verification_refs.iter().collect::<BTreeSet<_>>();
      if unique.len() != verification_refs.len() || verification_refs.len() > 32 {
        return Err(PlanError::Invalid("invalid verification references".into()));
      }
      for id in verification_refs {
        if !plan.verifications.iter().any(|record| {
          record.id == *id
            && record.step_id == *step_id
            && record.passed
            && record.acceptance == step.definition.acceptance
        }) {
          return Err(PlanError::VerificationRequired);
        }
      }
      if *state == StepState::Completed && user_override.is_none() {
        let latest = plan
          .verifications
          .iter()
          .rev()
          .find(|record| record.step_id == *step_id);
        if !latest.is_some_and(|record| record.passed && verification_refs.contains(&record.id)) {
          return Err(PlanError::VerificationRequired);
        }
      }
      if matches!(state, StepState::Blocked | StepState::Skipped) {
        text(reported_result, 16384)?;
      }
      let step = &mut plan.steps[index];
      step.state = *state;
      step.reported_result = reported_result.clone();
      step.verification_refs = verification_refs.clone();
      step.user_override = user_override.clone();
    }
  }
  plan.owner_epoch = context.owner_epoch;
  plan.revision = plan.revision.checked_add(1).ok_or(PlanError::Conflict)?;
  crate::validation::bounded_plan(&plan)?;
  Ok(plan)
}

pub fn record_verification(
  plan: &Plan,
  context: &PlanContext,
  expected_revision: u64,
  record: VerificationRecord,
) -> Result<Plan> {
  if plan.session_id != context.session_id || plan.run_id != context.run_id {
    return Err(PlanError::NotFound);
  }
  if plan.revision != expected_revision {
    return Err(PlanError::Conflict);
  }
  let step = plan
    .steps
    .iter()
    .find(|step| step.definition.id == record.step_id)
    .ok_or(PlanError::NotFound)?;
  if !matches!(step.state, StepState::Running | StepState::Blocked)
    || step.definition.acceptance != record.acceptance
    || record.id.is_nil()
    || plan
      .verifications
      .iter()
      .any(|previous| previous.id == record.id)
    || plan.verifications.len() >= 1024
  {
    return Err(PlanError::Invalid(
      "invalid verification identity or acceptance snapshot".into(),
    ));
  }
  text(&record.evidence_ref, 2048)?;
  text(&record.summary, 4096)?;
  let mut updated = plan.clone();
  updated.verifications.push(record);
  updated.owner_epoch = context.owner_epoch;
  updated.revision = updated.revision.checked_add(1).ok_or(PlanError::Conflict)?;
  crate::validation::bounded_plan(&updated)?;
  Ok(updated)
}
fn pending(definition: crate::StepDraft) -> PlanStep {
  PlanStep {
    definition,
    state: StepState::Pending,
    reported_result: String::new(),
    verification_refs: Vec::new(),
    user_override: None,
  }
}

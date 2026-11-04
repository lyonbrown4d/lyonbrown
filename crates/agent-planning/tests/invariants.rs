use agent_planning::*;
use uuid::Uuid;
fn fixture() -> (PlanContext, PlanMutation) {
  (
    PlanContext {
      session_id: "session".into(),
      run_id: "run".into(),
      owner_epoch: 1,
    },
    PlanMutation {
      request_id: Uuid::now_v7(),
      plan_id: Uuid::now_v7(),
      expected_revision: 0,
      change: PlanChange::Create {
        draft: PlanDraft {
          title: "Deliver".into(),
          steps: vec![StepDraft {
            id: Uuid::now_v7(),
            title: "Implement".into(),
            dependencies: vec![],
            acceptance: vec!["test passes".into()],
          }],
        },
      },
    },
  )
}
#[test]
fn model_report_cannot_complete_without_host_verification() -> Result<()> {
  let (context, create) = fixture();
  let plan = apply(None, &context, Actor::Model, &create)?;
  let mut update = PlanMutation {
    request_id: Uuid::now_v7(),
    plan_id: plan.id,
    expected_revision: 1,
    change: PlanChange::UpdateStep {
      step_id: plan.steps[0].definition.id,
      state: StepState::Running,
      reported_result: "".into(),
      verification_refs: vec![],
      user_override: None,
    },
  };
  let running = apply(Some(&plan), &context, Actor::Model, &update)?;
  update.expected_revision = 2;
  if let PlanChange::UpdateStep {
    state,
    reported_result,
    ..
  } = &mut update.change
  {
    *state = StepState::Completed;
    *reported_result = "all tests passed".into();
  }
  assert_eq!(
    apply(Some(&running), &context, Actor::Model, &update),
    Err(PlanError::VerificationRequired)
  );
  let record = VerificationRecord {
    id: Uuid::now_v7(),
    step_id: running.steps[0].definition.id,
    evidence_ref: "host:test-run/1".into(),
    acceptance: running.steps[0].definition.acceptance.clone(),
    passed: true,
    summary: "host observed test success".into(),
  };
  let verified = record_verification(&running, &context, 2, record.clone())?;
  update.expected_revision = 3;
  if let PlanChange::UpdateStep {
    verification_refs, ..
  } = &mut update.change
  {
    verification_refs.push(record.id);
  }
  let completed = apply(Some(&verified), &context, Actor::Model, &update)?;
  assert_eq!(completed.steps[0].state, StepState::Completed);
  assert_eq!(completed.steps[0].reported_result, "all tests passed");
  assert_eq!(completed.verifications, vec![record]);
  Ok(())
}
#[test]
fn rejects_cycle_and_model_user_override() -> Result<()> {
  let (context, mut create) = fixture();
  let plan = apply(None, &context, Actor::Model, &create)?;
  let override_mutation = PlanMutation {
    request_id: Uuid::now_v7(),
    plan_id: plan.id,
    expected_revision: 1,
    change: PlanChange::UpdateStep {
      step_id: plan.steps[0].definition.id,
      state: StepState::Running,
      reported_result: String::new(),
      verification_refs: vec![],
      user_override: None,
    },
  };
  let running = apply(Some(&plan), &context, Actor::Model, &override_mutation)?;
  let mut complete = override_mutation;
  complete.expected_revision = 2;
  if let PlanChange::UpdateStep {
    state,
    user_override,
    ..
  } = &mut complete.change
  {
    *state = StepState::Completed;
    *user_override = Some("User accepts manual inspection".into());
  }
  assert_eq!(
    apply(Some(&running), &context, Actor::Model, &complete),
    Err(PlanError::Forbidden)
  );
  assert_eq!(
    apply(Some(&running), &context, Actor::User, &complete)?.steps[0].state,
    StepState::Completed
  );
  if let PlanChange::Create { draft } = &mut create.change {
    let id = draft.steps[0].id;
    let second = Uuid::now_v7();
    draft.steps[0].dependencies.push(second);
    draft.steps.push(StepDraft {
      id: second,
      title: "Dependent".into(),
      dependencies: vec![id],
      acceptance: vec!["second criterion".into()],
    });
  }
  assert!(matches!(
    apply(None, &context, Actor::Model, &create),
    Err(PlanError::Invalid(_))
  ));
  Ok(())
}

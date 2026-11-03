use agent_planning::*;
use agent_storage::Store;
use uuid::Uuid;

#[tokio::test]
async fn durable_plan_cas_request_replay_and_event_outbox_are_atomic() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("planning.sqlite");
  let store = Store::open(&path).await?;
  let session = store.create_session("planning", "/tmp", "local").await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "implement", "request")
    .await?;
  let context = PlanContext {
    session_id: session.id.clone(),
    run_id: run.id.clone(),
    owner_epoch: 1,
  };
  let create = PlanMutation {
    request_id: Uuid::now_v7(),
    plan_id: Uuid::now_v7(),
    expected_revision: 0,
    change: PlanChange::Create {
      draft: PlanDraft {
        title: "Deliver".into(),
        steps: vec![StepDraft {
          id: Uuid::now_v7(),
          title: "Build".into(),
          dependencies: vec![],
          acceptance: vec!["host test passes".into()],
        }],
      },
    },
  };
  let before = store.events(&run.id, 0, 100).await?;
  let saved = store.plan_apply(&context, Actor::Model, &create).await?;
  assert_eq!(
    store.plan_apply(&context, Actor::Model, &create).await?,
    saved
  );
  let mut collision = create.clone();
  collision.expected_revision = 100;
  assert!(
    store
      .plan_apply(&context, Actor::Model, &collision)
      .await
      .is_err()
  );
  let mut stale = create.clone();
  stale.request_id = Uuid::now_v7();
  assert!(
    store
      .plan_apply(&context, Actor::Model, &stale)
      .await
      .is_err()
  );
  let mut wrong_epoch = context.clone();
  wrong_epoch.owner_epoch = 99;
  assert!(
    store
      .plan_apply(&wrong_epoch, Actor::Model, &create)
      .await
      .is_err()
  );
  let events = store.events(&run.id, 0, 100).await?;
  assert_eq!(events.len(), before.len() + 1);
  assert_eq!(
    events.last().map(|event| event.kind.as_str()),
    Some("plan_updated")
  );
  assert_eq!(store.pending_outbox(100).await?, events);
  let other = store.create_session("other", "/tmp", "local").await?;
  assert!(store.plan_get(&other.id, saved.id).await?.is_none());
  assert_eq!(
    store
      .plan_list(&context.session_id, Some(&run.id), None, 10)
      .await?
      .plans,
    vec![saved.clone()]
  );
  drop(store);
  let reopened = Store::open(&path).await?;
  assert_eq!(
    reopened.plan_get(&session.id, saved.id).await?,
    Some(saved.clone())
  );
  assert_eq!(
    reopened.plan_apply(&context, Actor::Model, &create).await?,
    saved
  );
  assert_eq!(reopened.events(&run.id, 0, 100).await?, events);
  Ok(())
}

#[tokio::test]
async fn completion_persists_host_receipt_separately_and_rejected_reports_leave_no_event()
-> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let store = Store::open(&directory.path().join("verification.sqlite")).await?;
  let session = store
    .create_session("verification", "/tmp", "local")
    .await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "verify", "verification")
    .await?;
  let context = PlanContext {
    session_id: session.id,
    run_id: run.id.clone(),
    owner_epoch: 1,
  };
  let step = StepDraft {
    id: Uuid::now_v7(),
    title: "Verify".into(),
    dependencies: vec![],
    acceptance: vec!["test result observed".into()],
  };
  let create = PlanMutation {
    request_id: Uuid::now_v7(),
    plan_id: Uuid::now_v7(),
    expected_revision: 0,
    change: PlanChange::Create {
      draft: PlanDraft {
        title: "Verification".into(),
        steps: vec![step.clone()],
      },
    },
  };
  store.plan_apply(&context, Actor::Model, &create).await?;
  let mut update = PlanMutation {
    request_id: Uuid::now_v7(),
    plan_id: create.plan_id,
    expected_revision: 1,
    change: PlanChange::UpdateStep {
      step_id: step.id,
      state: StepState::Running,
      reported_result: String::new(),
      verification_refs: vec![],
      user_override: None,
    },
  };
  store.plan_apply(&context, Actor::Model, &update).await?;
  update.request_id = Uuid::now_v7();
  update.expected_revision = 2;
  if let PlanChange::UpdateStep {
    state,
    reported_result,
    ..
  } = &mut update.change
  {
    *state = StepState::Completed;
    *reported_result = "Model claims success".into();
  }
  let before = store.events(&run.id, 0, 100).await?;
  assert!(
    store
      .plan_apply(&context, Actor::Model, &update)
      .await
      .is_err()
  );
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  assert_eq!(
    store
      .plan_get(&context.session_id, create.plan_id)
      .await?
      .expect("plan")
      .steps[0]
      .state,
    StepState::Running
  );
  let receipt = VerificationRecord {
    id: Uuid::now_v7(),
    step_id: step.id,
    evidence_ref: "host:run/test".into(),
    acceptance: step.acceptance,
    passed: true,
    summary: "Host observed acceptance test exit status".into(),
  };
  let request_id = Uuid::now_v7();
  let verified = store
    .plan_record_verification(&context, request_id, create.plan_id, 2, receipt.clone())
    .await?;
  assert_eq!(
    store
      .plan_record_verification(&context, request_id, create.plan_id, 2, receipt.clone())
      .await?,
    verified
  );
  update.expected_revision = 3;
  if let PlanChange::UpdateStep {
    verification_refs, ..
  } = &mut update.change
  {
    verification_refs.push(receipt.id);
  }
  let completed = store.plan_apply(&context, Actor::Model, &update).await?;
  assert_eq!(completed.steps[0].state, StepState::Completed);
  assert_eq!(completed.steps[0].reported_result, "Model claims success");
  assert_eq!(completed.verifications, vec![receipt]);
  assert_eq!(
    store.plan_get(&context.session_id, create.plan_id).await?,
    Some(completed)
  );
  assert_eq!(
    store.pending_outbox(100).await?,
    store.events(&run.id, 0, 100).await?
  );
  Ok(())
}

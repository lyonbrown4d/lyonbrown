use agent_planning::*;
use agent_storage::{OperationStatus, Store};
use serde_json::json;
use uuid::Uuid;
#[tokio::test]
async fn host_verification_rejects_earlier_commands_and_derives_failure_from_the_receipt()
-> anyhow::Result<()> {
  let data = tempfile::tempdir()?;
  let store = Store::open(&data.path().join("verify.sqlite")).await?;
  let session = store.create_session("verify", "/tmp", "local").await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "verify", "request")
    .await?;
  store
    .update_run_owned(&run.id, 1, agent_core::RunStatus::Running, "", "")
    .await?;
  let context = PlanContext {
    session_id: session.id,
    run_id: run.id.clone(),
    owner_epoch: 1,
  };
  let early = Uuid::now_v7().to_string();
  store
    .begin_operation(&run.id, 1, &early, "execute_command", json!({}))
    .await?;
  store
    .finish_operation(
      &run.id,
      1,
      &early,
      OperationStatus::Completed,
      json!({"exit_code":0}),
    )
    .await?;
  let step = StepDraft {
    id: Uuid::now_v7(),
    title: "Verify behavior".into(),
    dependencies: vec![],
    acceptance: vec!["acceptance command exits successfully".into()],
  };
  let create = PlanMutation {
    request_id: Uuid::now_v7(),
    plan_id: Uuid::now_v7(),
    expected_revision: 0,
    change: PlanChange::Create {
      draft: PlanDraft {
        title: "Evidence".into(),
        steps: vec![step.clone()],
      },
    },
  };
  store.plan_apply(&context, Actor::Model, &create).await?;
  let start = PlanMutation {
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
  store.plan_apply(&context, Actor::Model, &start).await?;
  let verify = |operation_id: String| PlanMutation {
    request_id: Uuid::now_v7(),
    plan_id: create.plan_id,
    expected_revision: 2,
    change: PlanChange::VerifyStep {
      step_id: step.id,
      operation_id,
    },
  };
  let before = store.events(&run.id, 0, 100).await?;
  assert!(
    store
      .plan_apply(&context, Actor::Model, &verify(early))
      .await
      .is_err()
  );
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  let failed = Uuid::now_v7().to_string();
  store
    .begin_operation(&run.id, 1, &failed, "execute_command", json!({}))
    .await?;
  store
    .finish_operation(
      &run.id,
      1,
      &failed,
      OperationStatus::Failed,
      json!({"exit_code":17}),
    )
    .await?;
  let request = verify(failed);
  let verified = store.plan_apply(&context, Actor::Model, &request).await?;
  assert!(!verified.verifications[0].passed);
  assert_eq!(
    store.plan_apply(&context, Actor::Model, &request).await?,
    verified
  );
  let complete = PlanMutation {
    request_id: Uuid::now_v7(),
    plan_id: create.plan_id,
    expected_revision: 3,
    change: PlanChange::UpdateStep {
      step_id: step.id,
      state: StepState::Completed,
      reported_result: "model says passed".into(),
      verification_refs: vec![verified.verifications[0].id],
      user_override: None,
    },
  };
  assert!(
    store
      .plan_apply(&context, Actor::Model, &complete)
      .await
      .is_err()
  );
  assert_eq!(
    store
      .plan_get(&context.session_id, create.plan_id)
      .await?
      .expect("plan")
      .steps[0]
      .state,
    StepState::Running
  );
  Ok(())
}

use agent_client::AgentClient;
use agent_protocol::v1;
use clap::Parser;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
#[tokio::test]
async fn typed_plan_rpc_authenticates_fences_scope_and_persists_explicit_user_completion()
-> anyhow::Result<()> {
  let data = tempfile::tempdir()?;
  let workspace = tempfile::tempdir()?;
  let mut config = agentd::config::Config::try_parse_from(["agentd"])?;
  config.data_dir = Some(data.path().into());
  config.api_base = Some("http://127.0.0.1:1".into());
  let daemon = agentd::Daemon::open(config).await?;
  let listener = TcpListener::bind("127.0.0.1:0").await?;
  let endpoint = format!("http://{}", listener.local_addr()?);
  let serving = daemon.clone();
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(agentd::client_service(serving.clone()))
      .add_service(agentd::workflow_service(serving))
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
  });
  let mut anonymous =
    v1::workflow_service_client::WorkflowServiceClient::connect(endpoint.clone()).await?;
  assert_eq!(
    anonymous
      .list_plans(v1::ListWorkflowPlansRequest::default())
      .await
      .expect_err("authentication")
      .code(),
    tonic::Code::Unauthenticated
  );
  let client = AgentClient::connect(&endpoint, daemon.auth_token()).await?;
  let session = daemon
    .store
    .create_session(
      "plans",
      workspace.path().to_string_lossy().as_ref(),
      &daemon.node_id,
    )
    .await?;
  let other = daemon
    .store
    .create_session("other", "/tmp", &daemon.node_id)
    .await?;
  let (run, _) = daemon
    .store
    .accept_run(&session.id, 1, "task", "request")
    .await?;
  let step_id = uuid::Uuid::now_v7().to_string();
  let request = v1::MutateWorkflowPlanRequest {
    session_id: session.id.clone(),
    run_id: run.id.clone(),
    request_id: uuid::Uuid::now_v7().to_string(),
    plan_id: uuid::Uuid::now_v7().to_string(),
    expected_revision: 0,
    change: Some(v1::mutate_workflow_plan_request::Change::Create(
      v1::WorkflowPlanDraft {
        title: "Implement".into(),
        steps: vec![v1::WorkflowStepDraft {
          id: step_id.clone(),
          title: "Build".into(),
          dependencies: vec![],
          acceptance: vec!["manual review".into()],
        }],
      },
    )),
  };
  let saved = client.mutate_plan(request.clone()).await?;
  assert_eq!(client.mutate_plan(request).await?, saved);
  assert!(client.get_plan(&other.id, &saved.id).await.is_err());
  let update = v1::MutateWorkflowPlanRequest {
    session_id: session.id.clone(),
    run_id: run.id.clone(),
    request_id: uuid::Uuid::now_v7().to_string(),
    plan_id: saved.id.clone(),
    expected_revision: 1,
    change: Some(v1::mutate_workflow_plan_request::Change::UpdateStep(
      v1::UpdateWorkflowStep {
        step_id,
        state: v1::WorkflowStepState::Completed as i32,
        reported_result: String::new(),
        verification_refs: vec![],
        user_override: Some("I reviewed the result manually".into()),
      },
    )),
  };
  let completed = client.mutate_plan(update.clone()).await?;
  assert_eq!(
    completed.steps[0].state,
    v1::WorkflowStepState::Completed as i32
  );
  assert_eq!(
    completed.steps[0].user_override.as_deref(),
    Some("I reviewed the result manually")
  );
  assert!(completed.verifications.is_empty());
  let mut stale = update;
  stale.request_id = uuid::Uuid::now_v7().to_string();
  assert!(client.mutate_plan(stale).await.is_err());
  assert_eq!(client.get_plan(&session.id, &saved.id).await?, completed);
  assert_eq!(
    daemon.store.pending_outbox(100).await?,
    daemon.store.events(&run.id, 0, 100).await?
  );
  daemon.shutdown().await;
  server.abort();
  Ok(())
}

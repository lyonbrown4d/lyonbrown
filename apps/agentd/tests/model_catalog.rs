use agent_client::AgentClient;
use agent_protocol::v1;
use clap::Parser;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_catalog_is_authenticated_and_selection_stays_session_scoped() {
  let data = tempfile::tempdir().unwrap();
  let workspace = tempfile::tempdir().unwrap();
  let mut config = agentd::config::Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(data.path().into());
  config.api_base = Some("http://127.0.0.1:1".into());
  config.provider_api_key = Some("synthetic-no-inference-key".into());
  let daemon = agentd::Daemon::open(config).await.unwrap();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let serving = daemon.clone();
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(agentd::client_service(serving.clone()))
      .add_service(agentd::model_service(serving))
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
      .unwrap();
  });
  let mut anonymous = v1::model_service_client::ModelServiceClient::connect(endpoint.clone())
    .await
    .unwrap();
  assert_eq!(
    anonymous
      .list_models(v1::SessionRequest::default())
      .await
      .unwrap_err()
      .code(),
    tonic::Code::Unauthenticated
  );
  let client = AgentClient::connect(&endpoint, daemon.auth_token())
    .await
    .unwrap();
  assert!(client.list_sessions().await.unwrap().is_empty());
  let catalog = client.list_models("").await.unwrap();
  assert!(catalog.iter().any(|model| model.id == "deepseek-chat"));
  assert!(catalog.iter().all(|model| !model.selected));
  let configured = catalog
    .iter()
    .find(|model| {
      model
        .executions
        .iter()
        .any(|execution| execution.id == "configured")
    })
    .unwrap();
  assert!(
    client
      .select_model("", 0, &configured.id, "configured")
      .await
      .is_err()
  );
  assert!(
    client.list_sessions().await.unwrap().is_empty(),
    "browsing cannot create a chat"
  );
  let missing = client.list_models("missing-session").await.unwrap_err();
  assert_eq!(
    missing
      .downcast_ref::<tonic::Status>()
      .map(tonic::Status::code),
    Some(tonic::Code::NotFound)
  );
  let session = client
    .create_session("catalog selection", workspace.path().to_str().unwrap())
    .await
    .unwrap();
  client
    .select_model(
      &session.id,
      session.owner_epoch,
      &configured.id,
      "configured",
    )
    .await
    .unwrap();
  let stored = daemon.store.model_binding(&session.id).unwrap().unwrap();
  assert_eq!(stored.model_id, configured.id);
  assert!(
    client
      .list_models(&session.id)
      .await
      .unwrap()
      .iter()
      .any(|model| model.id == configured.id && model.selected)
  );
  assert!(client.list_runs(&session.id).await.unwrap().is_empty());
  server.abort();
  daemon.shutdown().await;
}

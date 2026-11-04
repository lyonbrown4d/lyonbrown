use agent_client::AgentClient;
use agent_core::provider_profiles::RemoteProviderProtocol;
use agent_protocol::v1;
use agent_test_support::{MockModelServer, ScriptedReply};
use clap::Parser;
use serde_json::json;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
#[tokio::test]
async fn gemini_profile_round_trips_and_selects_the_native_runtime_binding() -> anyhow::Result<()> {
  let remote = MockModelServer::start(vec![ScriptedReply::status(
    200,
    json!({"models":[{"name":"models/fixture-model"}]}),
  )])
  .await?;
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
      .add_service(agentd::model_service(serving))
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
  });
  let client = AgentClient::connect(&endpoint, daemon.auth_token()).await?;
  let saved = client
    .put_provider_profile(v1::PutProviderProfileRequest {
      profile: Some(v1::ProviderProfile {
        id: uuid::Uuid::now_v7().to_string(),
        name: "Gemini fixture".into(),
        protocol: v1::RemoteProviderProtocol::GeminiGenerateContent as i32,
        base_url: remote.base_url().into(),
        model: "fixture-model".into(),
        revision: 0,
        credential_configured: false,
      }),
      credential: b"synthetic-key".into(),
    })
    .await?;
  assert_eq!(
    saved.protocol,
    v1::RemoteProviderProtocol::GeminiGenerateContent as i32
  );
  let stored = daemon
    .store
    .provider_profile(&saved.id)
    .await?
    .expect("saved profile");
  assert_eq!(
    stored.protocol,
    RemoteProviderProtocol::GeminiGenerateContent
  );
  assert_eq!(stored.runtime_provider(), "gemini");
  let probe = client
    .probe_provider_profile(v1::ProbeProviderProfileRequest {
      profile: Some(saved.clone()),
      credential: vec![],
    })
    .await?;
  assert!(probe.reachable, "{}", probe.message);
  assert_eq!(probe.model_ids, vec!["fixture-model"]);
  let session = daemon
    .store
    .create_session(
      "Gemini",
      workspace.path().to_string_lossy().as_ref(),
      &daemon.node_id,
    )
    .await?;
  let model = client
    .list_models(&session.id)
    .await?
    .into_iter()
    .find(|model| model.id == format!("profile:{}", saved.id))
    .expect("profile model");
  client
    .select_model(&session.id, 1, &model.id, &model.executions[0].id)
    .await?;
  let binding = daemon
    .store
    .model_binding(&session.id)
    .await?
    .expect("binding");
  assert_eq!(binding.provider, "gemini");
  assert_eq!(binding.model, "fixture-model");
  assert_eq!(
    remote.requests().await.len(),
    1,
    "only the explicit probe contacted the mock"
  );
  daemon.shutdown().await;
  server.abort();
  Ok(())
}

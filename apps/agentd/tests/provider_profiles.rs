use agent_client::AgentClient;
use agent_protocol::v1::{self, client_service_client::ClientServiceClient};
use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol, text_reply};
use clap::Parser;
use serde_json::json;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, transport::Channel};

struct Node {
  daemon: agentd::Daemon,
  rpc: ClientServiceClient<Channel>,
  client: AgentClient,
  _data: tempfile::TempDir,
  server: tokio::task::JoinHandle<()>,
}
impl Node {
  async fn start() -> Self {
    let data = tempfile::tempdir().unwrap();
    let mut config = agentd::config::Config::try_parse_from(["agentd"]).unwrap();
    config.data_dir = Some(data.path().into());
    config.api_base = Some("http://127.0.0.1:1".into());
    // A same-named Gateway entry must not override a selected profile's endpoint or key.
    config
      .gateway
      .models
      .push(agent_config::GatewayModelSettings {
        id: "fixture-model".into(),
        upstream_model: "fixture-model".into(),
        api_base: "http://127.0.0.1:1/v1".into(),
        api_key_env: None,
        context_tokens: 128_000,
        capabilities: ["text".into(), "tools".into()].into(),
      });
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
    Self {
      rpc: ClientServiceClient::connect(endpoint.clone())
        .await
        .unwrap(),
      client: AgentClient::connect(&endpoint, daemon.auth_token())
        .await
        .unwrap(),
      daemon,
      _data: data,
      server,
    }
  }
  fn auth<T>(&self, value: T) -> Request<T> {
    let mut request = Request::new(value);
    request.metadata_mut().insert(
      "authorization",
      format!("Bearer {}", self.daemon.auth_token())
        .parse()
        .unwrap(),
    );
    request
  }
  async fn save(
    &mut self,
    profile: v1::ProviderProfile,
    key: &[u8],
  ) -> Result<v1::ProviderProfile, tonic::Status> {
    let request = self.auth(v1::PutProviderProfileRequest {
      profile: Some(profile),
      credential: key.into(),
    });
    self
      .rpc
      .put_provider_profile(request)
      .await
      .map(|r| r.into_inner())
  }
  async fn probe(
    &mut self,
    profile: v1::ProviderProfile,
    key: &[u8],
  ) -> Result<v1::ProviderProbe, tonic::Status> {
    let request = self.auth(v1::ProbeProviderProfileRequest {
      profile: Some(profile),
      credential: key.into(),
    });
    self
      .rpc
      .probe_provider_profile(request)
      .await
      .map(|r| r.into_inner())
  }
  async fn close(self) {
    self.daemon.shutdown().await;
    self.server.abort();
  }
}
fn draft(url: &str, protocol: i32) -> v1::ProviderProfile {
  v1::ProviderProfile {
    id: uuid::Uuid::new_v4().to_string(),
    name: "Synthetic provider".into(),
    base_url: url.into(),
    model: "fixture-model".into(),
    protocol,
    ..Default::default()
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_selection_pins_immutable_credentials_and_real_adapter_for_all_protocols() {
  let mut node = Node::start().await;
  assert_eq!(
    node
      .rpc
      .list_provider_profiles(v1::Empty {})
      .await
      .unwrap_err()
      .code(),
    tonic::Code::Unauthenticated
  );
  let workspace = tempfile::tempdir().unwrap();
  for (wire, protocol) in [
    (1, StandardProtocol::Responses),
    (2, StandardProtocol::ChatCompletions),
    (3, StandardProtocol::Anthropic),
  ] {
    let model = MockModelServer::start(vec![
      ScriptedReply::status(
        200,
        json!({"data":[{"id":"fixture-model","display_name":"Fixture"}]}),
      ),
      text_reply(protocol, "synthetic profile output"),
    ])
    .await
    .unwrap();
    let base = if wire == 3 {
      model.base_url().to_owned()
    } else {
      format!("{}/v1", model.base_url())
    };
    let saved = node
      .save(draft(&base, wire), b"synthetic-original-key")
      .await
      .unwrap();
    assert_eq!(saved.revision, 1);
    assert!(saved.credential_configured);
    let before = node
      .daemon
      .store
      .provider_profile(&saved.id)
      .await
      .unwrap()
      .unwrap();
    let probe = node.probe(saved.clone(), b"").await.unwrap();
    assert!(probe.reachable, "{}", probe.message);
    assert_eq!(probe.model_ids, ["fixture-model"]);
    assert_eq!(
      node
        .daemon
        .store
        .provider_profile(&saved.id)
        .await
        .unwrap()
        .unwrap(),
      before
    );
    let mut rename = saved.clone();
    rename.name = "Renamed synthetic provider".into();
    let saved = node.save(rename, b"").await.unwrap();
    assert_eq!(
      node
        .daemon
        .store
        .provider_profile(&saved.id)
        .await
        .unwrap()
        .unwrap()
        .credential_id,
      before.credential_id
    );
    let session = node
      .client
      .create_session("profile binding", workspace.path().to_str().unwrap())
      .await
      .unwrap();
    let model_id = format!("profile:{}", saved.id);
    let execution_id = format!("profile:{}:{}", saved.id, saved.revision);
    let catalog = node.client.list_models(&session.id).await.unwrap();
    assert!(
      catalog
        .iter()
        .any(|m| m.id == model_id && m.state == "ready")
    );
    assert_eq!(
      model.requests().await.len(),
      1,
      "listing never probes providers"
    );
    node
      .client
      .select_model(&session.id, session.owner_epoch, &model_id, &execution_id)
      .await
      .unwrap();
    let pinned = node
      .daemon
      .store
      .model_binding(&session.id)
      .await
      .unwrap()
      .unwrap();
    let mut edit = saved.clone();
    edit.model = "edited-model".into();
    let edited = node.save(edit, b"synthetic-new-key").await.unwrap();
    let after = node
      .daemon
      .store
      .provider_profile(&saved.id)
      .await
      .unwrap()
      .unwrap();
    assert_ne!(after.credential_id, before.credential_id);
    assert_eq!(
      node
        .save(saved.clone(), b"stale-key")
        .await
        .unwrap_err()
        .code(),
      tonic::Code::Aborted
    );
    assert_eq!(
      node.probe(saved, b"").await.unwrap_err().code(),
      tonic::Code::Aborted
    );
    let request = node.auth(v1::RemoveProviderProfileRequest {
      id: edited.id,
      expected_revision: edited.revision,
    });
    node.rpc.remove_provider_profile(request).await.unwrap();
    let run = node
      .client
      .start_run(
        &session.id,
        "Return a synthetic greeting",
        &uuid::Uuid::new_v4().to_string(),
      )
      .await
      .unwrap();
    assert_eq!(
      serde_json::to_value(
        node
          .daemon
          .store
          .model_binding(&run.id)
          .await
          .unwrap()
          .unwrap()
      )
      .unwrap(),
      serde_json::to_value(pinned).unwrap()
    );
    let terminal = tokio::time::timeout(Duration::from_secs(10), async {
      let mut stream = node.client.watch_run(&run.id, 0).await.unwrap();
      while stream.message().await.unwrap().is_some() {}
      node.client.get_run(&run.id).await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(terminal.status, "completed", "{}", terminal.error);
    assert!(terminal.result.contains("synthetic profile output"));
    let requests = model.requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/v1/models");
    assert_eq!(requests[1].path, protocol.path());
    assert_eq!(requests[1].body["model"], "fixture-model");
    let header = if wire == 3 {
      "x-api-key"
    } else {
      "authorization"
    };
    assert!(
      requests
        .iter()
        .all(|r| r.headers[header].ends_with("synthetic-original-key"))
    );
  }
  let request = node.auth(v1::Empty {});
  assert!(
    node
      .rpc
      .list_provider_profiles(request)
      .await
      .unwrap()
      .into_inner()
      .profiles
      .is_empty()
  );
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probes_validate_and_bound_untrusted_responses_without_persisting_or_redirecting() {
  let mut node = Node::start().await;
  let destination = MockModelServer::start(vec![]).await.unwrap();
  let remote = MockModelServer::start(vec![
    ScriptedReply::status(302, json!({"secret":"synthetic-key"}))
      .header("location", format!("{}/v1/models", destination.base_url())),
    ScriptedReply::status(401, json!({"error":"synthetic-key"})),
    ScriptedReply::status(200, json!({"data":vec![json!({"id":"model"}); 1001]})),
    ScriptedReply::status(200, json!({"data":[],"padding":"x".repeat(1024*1024)})),
    ScriptedReply::status(200, json!({"data":[]})),
    ScriptedReply::stall(),
  ])
  .await
  .unwrap();
  for url in [
    "http://example.com",
    "https://user:synthetic-key@example.com",
    "https://example.com?key=synthetic-key",
    "https://example.com/#fragment",
  ] {
    assert_eq!(
      node
        .probe(draft(url, 1), b"synthetic-key")
        .await
        .unwrap_err()
        .code(),
      tonic::Code::InvalidArgument
    );
  }
  assert!(remote.requests().await.is_empty());
  for _ in 0..4 {
    let probe = node
      .probe(draft(remote.base_url(), 1), b"synthetic-key")
      .await
      .unwrap();
    assert!(!probe.reachable);
    assert!(probe.model_ids.is_empty());
    assert!(!probe.message.contains("synthetic-key"));
  }
  assert!(
    destination.requests().await.is_empty(),
    "redirect must not receive credentials"
  );
  let custom = format!("{}/custom/api", remote.base_url());
  assert!(
    node
      .probe(draft(&custom, 2), b"synthetic-key")
      .await
      .unwrap()
      .reachable
  );
  assert_eq!(remote.requests().await[4].path, "/custom/api/models");
  let started = std::time::Instant::now();
  assert!(
    !node
      .probe(draft(remote.base_url(), 1), b"synthetic-key")
      .await
      .unwrap()
      .reachable
  );
  assert!(started.elapsed() < Duration::from_secs(7));
  let request = node.auth(v1::Empty {});
  assert!(
    node
      .rpc
      .list_provider_profiles(request)
      .await
      .unwrap()
      .into_inner()
      .profiles
      .is_empty()
  );
  assert!(node.client.list_sessions().await.unwrap().is_empty());
  node.close().await;
}

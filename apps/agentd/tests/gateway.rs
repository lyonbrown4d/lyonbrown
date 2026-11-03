mod support;

use std::{collections::BTreeMap, fs};

use agentd::{Daemon, config::Config};
use axum::{
  body::Body,
  http::{Request, StatusCode, header},
};
use clap::Parser;
use tower::ServiceExt;

const MODEL_TOKEN: &str = "synthetic-model-access-key-at-least-32-bytes";

#[tokio::test]
async fn daemon_binary_serves_the_opt_in_model_endpoint() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let control = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
  let model = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
  let control_address = control.local_addr()?;
  let model_address = model.local_addr()?;
  drop((control, model));
  let path = directory.path().join("config.toml");
  fs::write(
    &path,
    format!("[gateway]\nhttp_listen='{model_address}'\nhttp_token_env='MODEL_TOKEN'\n"),
  )?;
  let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_agentd"))
    .env_clear()
    .env("MODEL_TOKEN", MODEL_TOKEN)
    .arg("--data-dir")
    .arg(directory.path())
    .arg("--config-file")
    .arg(&path)
    .args([
      "--listen",
      &control_address.to_string(),
      "--provider",
      "openai-completions",
      "--model",
      "synthetic-chat",
      "--api-base",
      "http://127.0.0.1:9/v1",
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true)
    .spawn()?;
  let client = reqwest::Client::builder()
    .no_proxy()
    .timeout(std::time::Duration::from_secs(1))
    .build()?;
  let url = format!("http://{model_address}/v1/models");
  let response = tokio::time::timeout(std::time::Duration::from_secs(15), async {
    loop {
      if let Some(status) = child.try_wait()? {
        anyhow::bail!("daemon exited before its model endpoint became ready: {status}");
      }
      if let Ok(response) = client.get(&url).bearer_auth(MODEL_TOKEN).send().await
        && response.status().is_success()
      {
        break Ok::<_, anyhow::Error>(response);
      }
      tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
  })
  .await??;
  let body = response.json::<serde_json::Value>().await?;
  assert_eq!(body["data"][0]["id"], "synthetic-chat");
  assert_eq!(
    client.get(&url).send().await?.status(),
    reqwest::StatusCode::UNAUTHORIZED
  );
  child.kill().await?;
  child.wait().await?;
  Ok(())
}

#[tokio::test]
async fn http_is_optional_and_model_credentials_cannot_be_replaced_by_daemon_credentials()
-> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let mut config = Config::try_parse_from(["agentd"])?;
  config.data_dir = Some(directory.path().into());
  config.provider = "openai-completions".into();
  config.model = "synthetic-chat".into();
  config.provider_api_key = Some("synthetic-upstream-key".into());
  config.api_base = Some("http://127.0.0.1:9/v1".into());
  let daemon = Daemon::open(config.clone()).await?;
  assert!(daemon.gateway_router()?.is_none());
  daemon.shutdown().await;
  drop(daemon);

  fs::write(
    directory.path().join("config.toml"),
    "[gateway]\nhttp_listen='127.0.0.1:7448'\nhttp_token_env='MODEL_TOKEN'\n",
  )?;
  let settings = agent_config::load_with_environment(
    directory.path(),
    None,
    &BTreeMap::from([("MODEL_TOKEN".into(), MODEL_TOKEN.into())]),
  )?;
  config.configure_gateway(&settings)?;
  let daemon = Daemon::open(config).await?;
  let router = daemon
    .gateway_router()?
    .ok_or_else(|| anyhow::anyhow!("missing router"))?;
  for token in [None, Some(daemon.auth_token()), Some(MODEL_TOKEN)] {
    let mut request = Request::get("/v1/models");
    if let Some(token) = token {
      request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = router.clone().oneshot(request.body(Body::empty())?).await?;
    assert_eq!(
      response.status(),
      if token == Some(MODEL_TOKEN) {
        StatusCode::OK
      } else {
        StatusCode::UNAUTHORIZED
      }
    );
  }
  daemon.shutdown().await;
  Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_and_same_id_gateway_executions_reach_their_selected_upstreams()
-> anyhow::Result<()> {
  use agent_client::AgentClient;
  use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
  use tokio_stream::wrappers::TcpListenerStream;

  let chat = StandardProtocol::ChatCompletions;
  let main = MockModelServer::start(vec![text_reply(chat, "default result")]).await?;
  let other = MockModelServer::start(vec![]).await?;
  let classifier = MockModelServer::start(vec![text_reply(chat, "invalid decision")]).await?;
  let node = support::TestNode::start_configured(&main, &classifier, chat, |config| {
    config.api_base = Some(format!("{}/v1", main.base_url()));
    config.decision.api_base = Some(format!("{}/v1", classifier.base_url()));
    config.gateway.models = vec![gateway_model(
      "other-model",
      "other-upstream",
      other.base_url(),
    )];
  })
  .await;
  let run = node.run("explain this workspace").await;
  let result = node.terminal(&run.id).await;
  assert_eq!(result.status, "completed", "{}", result.error);
  assert_eq!(result.result, "default result");
  let calls = main.requests().await;
  assert_eq!(calls.len(), 1);
  assert_eq!(calls[0].path, "/v1/chat/completions");
  assert_eq!(calls[0].body["model"], "main-fixture");
  assert_eq!(
    calls[0].headers.get("authorization").map(String::as_str),
    Some("Bearer synthetic-main-secret")
  );
  assert!(other.requests().await.is_empty());
  node.close().await;

  let responses = StandardProtocol::Responses;
  let configured = MockModelServer::start(vec![]).await?;
  let gateway = MockModelServer::start(vec![text_reply(chat, "gateway result")]).await?;
  let classifier = MockModelServer::start(vec![text_reply(responses, "invalid decision")]).await?;
  let node = support::TestNode::start_configured(&configured, &classifier, responses, |config| {
    config.api_base = Some(format!("{}/v1", configured.base_url()));
    config.decision.api_base = Some(format!("{}/v1", classifier.base_url()));
    config.gateway.models = vec![gateway_model(
      "main-fixture",
      "gateway-upstream",
      gateway.base_url(),
    )];
  })
  .await;
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
  let address = listener.local_addr()?;
  let service = agentd::model_service(node.daemon.clone());
  let client_service = agentd::client_service(node.daemon.clone());
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(client_service)
      .add_service(service)
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
  });
  let client = AgentClient::connect(&format!("http://{address}"), node.daemon.auth_token()).await?;
  let models = client.list_models(&node.session_id).await?;
  let matching: Vec<_> = models
    .iter()
    .filter(|model| model.id == "main-fixture")
    .collect();
  assert_eq!(matching.len(), 1);
  let ids: Vec<_> = matching[0]
    .executions
    .iter()
    .map(|execution| execution.id.as_str())
    .collect();
  assert!(ids.contains(&"configured"));
  assert!(ids.contains(&"gateway:main-fixture"));
  client
    .select_model(&node.session_id, 1, "main-fixture", "gateway:main-fixture")
    .await?;
  let run = node.run("explain this workspace").await;
  let result = node.terminal(&run.id).await;
  assert_eq!(result.status, "completed", "{}", result.error);
  assert_eq!(result.result, "gateway result");
  let calls = gateway.requests().await;
  assert_eq!(calls.len(), 1);
  assert_eq!(calls[0].path, "/v1/chat/completions");
  assert_eq!(calls[0].body["model"], "gateway-upstream");
  assert!(configured.requests().await.is_empty());
  server.abort();
  node.close().await;
  Ok(())
}

fn gateway_model(id: &str, upstream: &str, base: &str) -> agent_config::GatewayModelSettings {
  agent_config::GatewayModelSettings {
    id: id.into(),
    upstream_model: upstream.into(),
    api_base: format!("{base}/v1"),
    api_key_env: None,
    context_tokens: 128_000,
    capabilities: ["text".into(), "tools".into()].into(),
  }
}

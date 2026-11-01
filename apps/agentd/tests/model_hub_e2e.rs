use agent_client::AgentClient;
use agent_protocol::v1;
use axum::{Json, Router, extract::Request, response::IntoResponse, routing::any};
use clap::Parser;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

const REV: &str = "0123456789012345678901234567890123456789";
const WEIGHTS: &[u8] = b"GGUF-synthetic-model-bytes";

async fn hub(request: Request) -> axum::response::Response {
  let path = request.uri().path();
  if path.contains("/resolve/") {
    return ([("content-length", WEIGHTS.len().to_string())], WEIGHTS).into_response();
  }
  let info = json!({"id":"test/model","sha":REV,"gated":false,"config":{"model_type":"llama"}});
  if path == "/api/models" {
    return Json(json!([info])).into_response();
  }
  if path.contains("/tree/") {
    return Json(json!([{"type":"file","oid":"fixture","path":"model.gguf","size":WEIGHTS.len(),"lfs":{"sha256":hex::encode(Sha256::digest(WEIGHTS)),"size":WEIGHTS.len(),"pointer_size":100}}])).into_response();
  }
  Json(info).into_response()
}

fn operation(action: &str, id: &str) -> v1::HubRequest {
  v1::HubRequest {
    action: action.into(),
    repository: "test/model".into(),
    revision: REV.into(),
    file: "model.gguf".into(),
    runtime: "llama_cpp".into(),
    id: id.into(),
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn authenticated_hub_install_persists_artifacts_and_never_claims_unloaded_model_ready() {
  let hf = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let hf_endpoint = format!("http://{}", hf.local_addr().unwrap());
  let hf_task = tokio::spawn(async move {
    axum::serve(hf, Router::new().fallback(any(hub)))
      .await
      .unwrap();
  });
  let data = tempfile::tempdir().unwrap();
  let workspace = tempfile::tempdir().unwrap();
  let mut config = agentd::config::Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(data.path().into());
  config.hub_endpoint = hf_endpoint;
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
  let client = AgentClient::connect(&endpoint, daemon.auth_token())
    .await
    .unwrap();
  let session = client
    .create_session("hub", workspace.path().to_str().unwrap())
    .await
    .unwrap();
  let search = client.hub_operation(operation("search", "")).await.unwrap();
  assert!(search.contains("test/model"));
  let planned: Value =
    serde_json::from_str(&client.hub_operation(operation("plan", "")).await.unwrap()).unwrap();
  let id = planned["id"].as_str().unwrap();
  assert!(
    client
      .hub_operation(operation("install", "incorrect-plan"))
      .await
      .is_err()
  );
  client
    .hub_operation(operation("install", id))
    .await
    .unwrap();
  tokio::time::timeout(std::time::Duration::from_secs(15), async {
    loop {
      let status: Value =
        serde_json::from_str(&client.hub_operation(operation("status", id)).await.unwrap())
          .unwrap();
      if status["state"] == "installed_unverified" {
        break;
      }
      assert_ne!(status["state"], "failed", "{status}");
      tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
  })
  .await
  .unwrap();
  let records = client.list_models(&session.id).await.unwrap();
  assert!(
    records
      .iter()
      .any(|model| model.id == id && model.state == "installed_unverified")
  );
  // A synthetic header is never a valid native weight fixture.
  assert!(client.hub_operation(operation("load", id)).await.is_err());
  assert!(
    client
      .select_model(&session.id, session.owner_epoch, id, &format!("local:{id}"))
      .await
      .is_err()
  );
  assert_eq!(
    daemon.store.model_job(id).unwrap().unwrap()["state"],
    "installed_unverified"
  );
  client.hub_operation(operation("remove", id)).await.unwrap();
  let installed: Value = serde_json::from_str(
    &client
      .hub_operation(operation("installed", ""))
      .await
      .unwrap(),
  )
  .unwrap();
  assert!(installed.as_array().unwrap().is_empty());
  daemon.shutdown().await;
  server.abort();
  hf_task.abort();
}

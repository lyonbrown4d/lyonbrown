mod support;

use agent_protocol::v1;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use std::time::Duration;
use support::TestNode;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protects_run_and_queue_before_storage_and_models() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, "invalid decision")])
    .await
    .unwrap();
  let main = MockModelServer::start_interactive().await.unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  assert!(
    agent_client::AgentClient::connect_authenticated(
      &format!("http://{}", node.daemon.config.listen),
      "wrong-token",
    )
    .await
    .is_err()
  );
  let secret = "synthetic-main-secret";
  let prompt = format!("Explain this credential {secret} and email test@example.com");
  assert!(
    node
      .client
      .start_run_owned(&node.session_id, 2, &prompt, &agent_core::new_id())
      .await
      .is_err()
  );
  let key = agent_core::new_id();
  let run = node
    .client
    .start_run_owned(&node.session_id, 1, &prompt, &key)
    .await
    .unwrap();
  assert!(!run.prompt.contains(secret));
  assert!(!run.prompt.contains("test@example.com"));
  assert_eq!(
    node
      .client
      .start_run_owned(&node.session_id, 1, &prompt, &key)
      .await
      .unwrap()
      .id,
    run.id
  );
  tokio::time::timeout(Duration::from_secs(5), main.wait_for_requests(1))
    .await
    .unwrap();
  let queued = v1::RunInputRequest {
    run_id: run.id.clone(),
    owner_epoch: 1,
    request_id: agent_core::new_id(),
    kind: v1::RunInputKind::FollowUp as i32,
    prompt: prompt.clone(),
  };
  let mut stale = queued.clone();
  stale.owner_epoch = 2;
  assert!(node.client.submit_run_input(stale).await.is_err());
  let receipt = node.client.submit_run_input(queued.clone()).await.unwrap();
  assert_eq!(receipt, node.client.submit_run_input(queued).await.unwrap());
  assert!(!receipt.prompt.contains(secret));
  let snapshot = node
    .daemon
    .store
    .load_snapshot(&node.session_id)
    .await
    .unwrap()
    .unwrap();
  let snapshot = String::from_utf8(snapshot).unwrap();
  assert!(!snapshot.contains(secret));
  assert!(!snapshot.contains("test@example.com"));
  let events = node.daemon.store.events(&run.id, 0, 1000).await.unwrap();
  assert!(
    events
      .iter()
      .all(|event| !event.payload_json.contains(secret)
        && !event.payload_json.contains("test@example.com"))
  );
  for request in classifier
    .requests()
    .await
    .into_iter()
    .chain(main.requests().await)
  {
    let body = request.body.to_string();
    assert!(!body.contains(secret));
    assert!(!body.contains("test@example.com"));
  }
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protected_run_retry_remains_idempotent_after_restart() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, "invalid decision")])
    .await
    .unwrap();
  let main = MockModelServer::start(vec![text_reply(protocol, "finished")])
    .await
    .unwrap();
  let data = tempfile::tempdir().unwrap();
  let node = TestNode::start_configured(&main, &classifier, protocol, |config| {
    config.data_dir = Some(data.path().to_owned())
  })
  .await;
  let session = node.session_id.clone();
  let key = agent_core::new_id();
  let prompt = "inspect synthetic-main-secret and test@example.com";
  let run = node
    .client
    .start_run_owned(&session, 1, prompt, &key)
    .await
    .unwrap();
  assert_eq!(node.terminal(&run.id).await.status, "completed");
  node.close_and_wait().await;
  let node = TestNode::start_configured(&main, &classifier, protocol, |config| {
    config.data_dir = Some(data.path().to_owned())
  })
  .await;
  let retry = node
    .client
    .start_run_owned(&session, 1, prompt, &key)
    .await
    .unwrap();
  assert_eq!(retry.id, run.id);
  assert_eq!(retry.prompt, run.prompt);
  assert_eq!(main.requests().await.len(), 1);
  node.close_and_wait().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unselected_uploaded_and_rotated_credentials_are_protected_across_restart() {
  use agent_test_support::tool_reply;
  use serde_json::json;

  let protocol = StandardProtocol::ChatCompletions;
  let route = json!({"task_kind":"code","model_policy":"general","allowed_tools":["read_file"],"execution_mode":"local","target_hint":null,"risk_level":"low","confidence":0.99}).to_string();
  let classifier = MockModelServer::start(vec![
    text_reply(protocol, &route),
    text_reply(protocol, &route),
  ])
  .await
  .unwrap();
  let main = MockModelServer::start(vec![
    tool_reply(protocol, "read_file", json!({"path":"evidence.txt"})),
    text_reply(protocol, "safe result"),
    tool_reply(protocol, "read_file", json!({"path":"evidence.txt"})),
    text_reply(protocol, "safe result"),
  ])
  .await
  .unwrap();
  let data = tempfile::tempdir().unwrap();
  let secrets = [
    "ambercopperwillowmeadow184673",
    "violetmarbleharborforest593812",
    "indigosilvercanyonprairie471239",
    "cedargoldmeadowisland258731",
  ];
  let node = TestNode::start_configured(&main, &classifier, protocol, |config| {
    config.data_dir = Some(data.path().to_owned());
  })
  .await;
  node
    .client
    .put_provider_credential("anthropic", secrets[0].as_bytes().to_vec())
    .await
    .unwrap();
  node
    .client
    .put_provider_credential("anthropic", secrets[1].as_bytes().to_vec())
    .await
    .unwrap();
  let profile = v1::ProviderProfile {
    id: uuid::Uuid::new_v4().to_string(),
    name: "Unselected fixture".into(),
    protocol: v1::RemoteProviderProtocol::OpenaiCompletions as i32,
    base_url: "http://127.0.0.1:9/v1".into(),
    model: "other-model".into(),
    ..Default::default()
  };
  let profile = node
    .client
    .put_provider_profile(v1::PutProviderProfileRequest {
      profile: Some(profile),
      credential: secrets[2].as_bytes().to_vec(),
    })
    .await
    .unwrap();
  node
    .client
    .put_provider_profile(v1::PutProviderProfileRequest {
      profile: Some(profile),
      credential: secrets[3].as_bytes().to_vec(),
    })
    .await
    .unwrap();
  exercise_unselected_secrets(&node, &main, &classifier, &secrets, 0).await;
  node.close_and_wait().await;

  let node = TestNode::start_configured(&main, &classifier, protocol, |config| {
    config.data_dir = Some(data.path().to_owned());
  })
  .await;
  exercise_unselected_secrets(&node, &main, &classifier, &secrets, 2).await;
  node.close_and_wait().await;
}

async fn exercise_unselected_secrets(
  node: &TestNode,
  main: &MockModelServer,
  classifier: &MockModelServer,
  secrets: &[&str],
  previous_requests: usize,
) {
  let content = secrets.join(" ");
  std::fs::write(node.workspace.path().join("evidence.txt"), &content).unwrap();
  let run = node.run(&format!("Inspect evidence for {content}")).await;
  let terminal = node.terminal(&run.id).await;
  assert_eq!(terminal.status, "completed", "{}", terminal.error);
  assert_eq!(terminal.result, "safe result");
  let requests = main.requests().await;
  assert_eq!(requests.len(), previous_requests + 2);
  assert!(
    requests[previous_requests + 1]
      .body
      .to_string()
      .contains("[PRIVATE:")
  );
  let snapshot = node
    .daemon
    .store
    .load_snapshot(&node.session_id)
    .await
    .unwrap()
    .unwrap();
  let events = node.daemon.store.events(&run.id, 0, 1000).await.unwrap();
  let run = node.client.get_run(&run.id).await.unwrap();
  let persisted = format!(
    "{}{}{}",
    run.prompt,
    String::from_utf8(snapshot).unwrap(),
    serde_json::to_string(&events).unwrap()
  );
  let classifier_requests = classifier.requests().await;
  for &secret in secrets {
    assert!(
      !persisted.contains(secret),
      "credential reached persisted state"
    );
    for request in requests.iter().chain(&classifier_requests) {
      assert!(
        !request.body.to_string().contains(secret),
        "credential reached model input"
      );
    }
    for filename in [
      "agent.sqlite3",
      "agent.sqlite3-wal",
      "gateway.sqlite",
      "gateway.sqlite-wal",
    ] {
      match std::fs::read(node.daemon.config.data_path().unwrap().join(filename)) {
        Ok(bytes) => assert!(
          !bytes
            .windows(secret.len())
            .any(|window| window == secret.as_bytes()),
          "credential reached database storage"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("database read failed: {error}"),
      }
    }
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_name_containing_new_credential_is_rejected_before_any_secret_write() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![]).await.unwrap();
  let main = MockModelServer::start(vec![]).await.unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  let directory = node
    .daemon
    .config
    .data_path()
    .unwrap()
    .join("provider-secrets");
  let before = encrypted_store_contents(&directory);
  let secret = "opalharborforestcopper672941";
  let profile = v1::ProviderProfile {
    id: uuid::Uuid::new_v4().to_string(),
    name: secret.into(),
    protocol: v1::RemoteProviderProtocol::OpenaiCompletions as i32,
    base_url: "http://127.0.0.1:9/v1".into(),
    model: "other-model".into(),
    ..Default::default()
  };
  let error = node
    .client
    .put_provider_profile(v1::PutProviderProfileRequest {
      profile: Some(profile),
      credential: secret.as_bytes().to_vec(),
    })
    .await
    .expect_err("profile name must not contain a credential");
  assert!(
    error
      .downcast_ref::<tonic::Status>()
      .is_some_and(|status| status.code() == tonic::Code::InvalidArgument)
  );
  assert!(!error.to_string().contains(secret));
  assert!(
    node
      .client
      .list_provider_profiles()
      .await
      .unwrap()
      .is_empty()
  );
  assert!(
    before == encrypted_store_contents(&directory),
    "rejected profile must not write the secret store"
  );
  node.close().await;
}

fn encrypted_store_contents(
  directory: &std::path::Path,
) -> std::collections::BTreeMap<std::ffi::OsString, Vec<u8>> {
  std::fs::read_dir(directory)
    .unwrap()
    .map(|entry| {
      let entry = entry.unwrap();
      (entry.file_name(), std::fs::read(entry.path()).unwrap())
    })
    .collect()
}

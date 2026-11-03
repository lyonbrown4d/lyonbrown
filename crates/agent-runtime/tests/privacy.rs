mod support;
use agent_runtime::{RunOutcome, Runtime, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use agent_test_support::{StandardProtocol, tool_reply};
use serde_json::json;
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
use support::{Server, config, shared, text, tool};
use tokio_util::sync::CancellationToken;

struct Inspect(Arc<AtomicUsize>, bool);
#[async_trait::async_trait]
impl RuntimeTool for Inspect {
  fn definition(&self) -> ToolDefinition {
    ToolDefinition {
      name: "inspect".into(),
      description: "inspect".into(),
      parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
    }
  }
  fn authorize_sensitive_reference(&self, _: &ToolCall, _: &str) -> bool {
    self.1
  }
  async fn execute(&self, _: ToolCall, _: CancellationToken) -> agent_runtime::Result<ToolOutput> {
    self.0.fetch_add(1, Ordering::SeqCst);
    Ok(ToolOutput {
      text: "email person@example.test".into(),
      is_error: false,
    })
  }
}

#[tokio::test]
async fn input_and_tool_output_are_protected_before_upstream_and_persistence() {
  let mut server = Server::start(vec![tool(), text("done")]).await;
  let store = shared();
  let calls = Arc::new(AtomicUsize::new(0));
  let runtime = Runtime::open(
    config(&server.endpoint),
    store.clone(),
    vec![Arc::new(Inspect(calls.clone(), false))],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  assert_eq!(
    runtime
      .execute("contact user@example.test")
      .await
      .unwrap()
      .outcome,
    RunOutcome::Completed
  );
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  let first = server.request().await.to_string();
  let second = server.request().await.to_string();
  assert!(!first.contains("user@example.test"));
  assert!(!second.contains("person@example.test"));
  assert!(first.contains("[PRIVATE:"));
  assert!(second.contains("[PRIVATE:"));
  let snapshot = String::from_utf8(store.snapshot.lock().unwrap().clone().unwrap()).unwrap();
  let events = serde_json::to_string(&*store.mutations.lock().unwrap()).unwrap();
  for value in [snapshot, events] {
    assert!(!value.contains("user@example.test"));
    assert!(!value.contains("person@example.test"));
  }
}

#[tokio::test]
async fn unapproved_placeholder_never_reaches_tool() {
  let reply = tool_reply(
    StandardProtocol::ChatCompletions,
    "inspect",
    json!({"path":"[PRIVATE:unknown]"}),
  );
  let server = Server::start(vec![reply]).await;
  let calls = Arc::new(AtomicUsize::new(0));
  let runtime = Runtime::open(
    config(&server.endpoint),
    shared(),
    vec![Arc::new(Inspect(calls.clone(), false))],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  assert!(matches!(
    runtime.execute("inspect").await.unwrap().outcome,
    RunOutcome::Failed { .. }
  ));
  assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn approved_but_unknown_reference_is_not_executed() {
  let reply = tool_reply(
    StandardProtocol::ChatCompletions,
    "inspect",
    json!({"path":"[PRIVATE:unknown]"}),
  );
  let server = Server::start(vec![reply]).await;
  let calls = Arc::new(AtomicUsize::new(0));
  let runtime = Runtime::open(
    config(&server.endpoint),
    shared(),
    vec![Arc::new(Inspect(calls.clone(), true))],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  assert!(matches!(
    runtime.execute("inspect").await.unwrap().outcome,
    RunOutcome::Failed { .. }
  ));
  assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sensitive_model_response_is_never_emitted_or_committed() {
  let server = Server::start(vec![text("private person@example.test")]).await;
  let store = shared();
  let events = Arc::new(std::sync::Mutex::new(Vec::new()));
  let capture = events.clone();
  let runtime = Runtime::open(
    config(&server.endpoint),
    store.clone(),
    vec![],
    Arc::new(move |event| capture.lock().unwrap().push(event)),
  )
  .await
  .unwrap();
  assert!(matches!(
    runtime.execute("hello").await.unwrap().outcome,
    RunOutcome::Failed { .. }
  ));
  let delivered = serde_json::to_string(&*events.lock().unwrap()).unwrap();
  let snapshot = String::from_utf8(store.snapshot.lock().unwrap().clone().unwrap()).unwrap();
  assert!(!delivered.contains("person@example.test"));
  assert!(!snapshot.contains("person@example.test"));
}

#[derive(Default)]
struct PrivateMemory {
  public: support::Memory,
  scope: std::sync::Mutex<Option<secrecy::SecretString>>,
  fail_private_write: std::sync::atomic::AtomicBool,
}
#[async_trait::async_trait]
impl agent_runtime::SessionPersistence for PrivateMemory {
  async fn load_private_scope(
    &self,
    _: &str,
  ) -> agent_runtime::Result<Option<secrecy::SecretString>> {
    Ok(self.scope.lock().unwrap().clone())
  }
  async fn commit_private_scope(
    &self,
    _: &str,
    _: u64,
    scope: secrecy::SecretString,
  ) -> agent_runtime::Result<()> {
    if self.fail_private_write.load(Ordering::SeqCst) {
      return Err(agent_runtime::RuntimeError::Persistence(
        "private write failed".into(),
      ));
    }
    *self.scope.lock().unwrap() = Some(scope);
    Ok(())
  }
  async fn load(&self, id: &str) -> agent_runtime::Result<Option<Vec<u8>>> {
    self.public.load(id).await
  }
  async fn commit(
    &self,
    id: &str,
    epoch: u64,
    snapshot: Vec<u8>,
    mutation: agent_runtime::SessionMutation,
  ) -> agent_runtime::Result<()> {
    self.public.commit(id, epoch, snapshot, mutation).await
  }
}

#[tokio::test]
async fn private_scope_restores_stable_references_after_restart() {
  use secrecy::ExposeSecret;
  let mut server = Server::start(vec![text("done"), text("done again")]).await;
  let store = Arc::new(PrivateMemory::default());
  let config = config(&server.endpoint);
  let runtime = Runtime::open(config.clone(), store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap();
  runtime.execute("contact user@example.test").await.unwrap();
  let first = store.scope.lock().unwrap().clone().unwrap();
  drop(runtime);
  let runtime = Runtime::open(config, store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap();
  runtime.execute("contact user@example.test").await.unwrap();
  let second = store.scope.lock().unwrap().clone().unwrap();
  assert_eq!(first.expose_secret(), second.expose_secret());
  let first_request = server.request().await.to_string();
  let second_request = server.request().await.to_string();
  assert!(!first_request.contains("user@example.test"));
  assert!(!second_request.contains("user@example.test"));
}

struct AuthorizedSink(Arc<std::sync::Mutex<Option<serde_json::Value>>>, bool);
#[async_trait::async_trait]
impl RuntimeTool for AuthorizedSink {
  fn definition(&self) -> ToolDefinition {
    ToolDefinition {
      name: "inspect".into(),
      description: "approved sink".into(),
      parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
    }
  }
  fn authorize_sensitive_reference(&self, _: &ToolCall, _: &str) -> bool {
    true
  }
  fn authorize_resolved_sensitive_call(&self, _: &ToolCall, resolved: &ToolCall) -> bool {
    self.1 && resolved.arguments == json!({"path":"user@example.test"})
  }
  async fn execute(
    &self,
    call: ToolCall,
    _: CancellationToken,
  ) -> agent_runtime::Result<ToolOutput> {
    *self.0.lock().unwrap() = Some(call.arguments);
    Ok(ToolOutput {
      text: "ok".into(),
      is_error: false,
    })
  }
}

#[tokio::test]
async fn approved_sink_receives_original_only_after_host_resolution() {
  let server = agent_test_support::MockModelServer::start_interactive()
    .await
    .unwrap();
  let store = shared();
  let received = Arc::new(std::sync::Mutex::new(None));
  let runtime = Arc::new(
    Runtime::open(
      config(server.base_url()),
      store.clone(),
      vec![Arc::new(AuthorizedSink(received.clone(), true))],
      Arc::new(|_| {}),
    )
    .await
    .unwrap(),
  );
  let execution = tokio::spawn(async move { runtime.execute("user@example.test").await });
  server.wait_for_requests(1).await;
  let request = server.requests().await[0].body.to_string();
  let start = request.find("[PRIVATE:").unwrap();
  let token = &request[start..start + 42];
  server
    .push_reply(tool_reply(
      StandardProtocol::ChatCompletions,
      "inspect",
      json!({"path":token}),
    ))
    .await;
  server.wait_for_requests(2).await;
  server.push_reply(text("done")).await;
  assert_eq!(
    execution.await.unwrap().unwrap().outcome,
    RunOutcome::Completed
  );
  assert_eq!(
    *received.lock().unwrap(),
    Some(json!({"path":"user@example.test"}))
  );
  assert!(
    !String::from_utf8(store.snapshot.lock().unwrap().clone().unwrap())
      .unwrap()
      .contains("user@example.test")
  );
}

#[tokio::test]
async fn rejected_resolved_destination_never_reaches_tool() {
  let server = agent_test_support::MockModelServer::start_interactive()
    .await
    .unwrap();
  let received = Arc::new(std::sync::Mutex::new(None));
  let runtime = Arc::new(
    Runtime::open(
      config(server.base_url()),
      shared(),
      vec![Arc::new(AuthorizedSink(received.clone(), false))],
      Arc::new(|_| {}),
    )
    .await
    .unwrap(),
  );
  let execution = tokio::spawn(async move { runtime.execute("user@example.test").await });
  server.wait_for_requests(1).await;
  let request = server.requests().await[0].body.to_string();
  let start = request.find("[PRIVATE:").unwrap();
  server
    .push_reply(tool_reply(
      StandardProtocol::ChatCompletions,
      "inspect",
      json!({"path": &request[start..start + 42]}),
    ))
    .await;
  assert!(matches!(
    execution.await.unwrap().unwrap().outcome,
    RunOutcome::Failed { .. }
  ));
  assert!(received.lock().unwrap().is_none());
}

#[tokio::test]
async fn summary_boundary_protects_inputs_and_rejects_observed_secrets() {
  let runtime = Runtime::open(
    config("http://127.0.0.1:1"),
    shared(),
    vec![],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  let (system, transcript) = runtime
    .protect_summary_input("password=shortvalue", "person@example.test")
    .await
    .unwrap();
  assert!(!system.contains("shortvalue"));
  assert!(!transcript.contains("person@example.test"));
  assert!(runtime.validate_summary_output("shortvalue").await.is_err());
  assert!(
    runtime
      .validate_summary_output("a safe summary")
      .await
      .is_ok()
  );
}

#[tokio::test]
async fn shared_known_secret_is_removed_from_system_prompt_before_model_request() {
  use agent_test_support::text_reply;
  for protocol in [StandardProtocol::Responses, StandardProtocol::Anthropic] {
    let mut server = Server::start(vec![text_reply(protocol, "done")]).await;
    let store = shared();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let capture = events.clone();
    let mut config = config(&server.endpoint);
    config.provider = protocol.provider().into();
    config.system_prompt = Some("shared value violet-orchard-seven".into());
    let mut privacy = agent_privacy::PrivacyService::new(&[]).unwrap();
    privacy
      .register_secret(&config.session_id, "violet-orchard-seven")
      .unwrap();
    let runtime = Runtime::open(
      config,
      store.clone(),
      vec![],
      Arc::new(move |event| capture.lock().unwrap().push(event)),
    )
    .await
    .unwrap()
    .with_privacy(Arc::new(tokio::sync::Mutex::new(privacy)))
    .await
    .unwrap();
    assert_eq!(
      runtime.execute("hello").await.unwrap().outcome,
      RunOutcome::Completed
    );
    let request = server.request().await.to_string();
    assert!(request.contains("[PRIVATE:"));
    assert!(!request.contains("violet-orchard-seven"));
    let snapshot = String::from_utf8(store.snapshot.lock().unwrap().clone().unwrap()).unwrap();
    let persisted = serde_json::to_string(&*store.mutations.lock().unwrap()).unwrap();
    let delivered = serde_json::to_string(&*events.lock().unwrap()).unwrap();
    for contents in [snapshot, persisted, delivered] {
      assert!(!contents.contains("violet-orchard-seven"));
    }
  }
}

#[tokio::test]
async fn private_scope_persistence_failure_fences_the_writer() {
  let store = Arc::new(PrivateMemory::default());
  let runtime = Runtime::open(
    config("http://127.0.0.1:1"),
    store.clone(),
    vec![],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  store.fail_private_write.store(true, Ordering::SeqCst);
  assert!(matches!(
    runtime
      .protect_summary_input("hello", "person@example.test")
      .await,
    Err(agent_runtime::RuntimeError::Uncertain(_))
  ));
  assert!(runtime.input_persistence_failed());
  assert!(matches!(
    runtime.execute("hello").await,
    Err(agent_runtime::RuntimeError::Uncertain(_))
  ));
}

#[tokio::test]
async fn shared_detector_rejects_existing_snapshot_with_known_secret() {
  let server = Server::start(vec![text("done")]).await;
  let store = shared();
  let config = config(&server.endpoint);
  let runtime = Runtime::open(config.clone(), store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap();
  runtime.execute("violet-orchard-seven").await.unwrap();
  drop(runtime);
  let runtime = Runtime::open(config.clone(), store, vec![], Arc::new(|_| {}))
    .await
    .unwrap();
  let mut privacy = agent_privacy::PrivacyService::new(&[]).unwrap();
  privacy
    .register_secret(&config.session_id, "violet-orchard-seven")
    .unwrap();
  assert!(
    runtime
      .with_privacy(Arc::new(tokio::sync::Mutex::new(privacy)))
      .await
      .is_err()
  );
}

use super::{MockModelServer, Pair, StandardProtocol, json, local_support};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_uses_its_daemon_local_registry_without_cloud_fallback() {
  let unused_cloud = MockModelServer::start(vec![]).await.unwrap();
  let backend = local_support::Backend::new("worker-fixture", false, Some("local worker output"));
  let registry = std::sync::Arc::new(agent_inference::ModelRegistry::new(1).unwrap());
  registry.register(backend.clone(), 1).await.unwrap();
  let mut pair = Pair::open_with_models(
    &unused_cloud,
    StandardProtocol::ChatCompletions,
    Some(registry),
  )
  .await;
  pair.serve().await;
  let task = pair.envelope(vec!["workspace.read".into()]);
  pair
    .dispatch
    .submit(&pair.worker_manager.local_node(), task.clone())
    .await
    .unwrap();
  let result = pair.terminal(&task.task_id).await;
  assert_eq!(result.attempt_status, "succeeded", "{}", result.error);
  assert_eq!(result.status, "awaiting_acceptance");
  assert_eq!(result.result, "local worker output");
  assert!(unused_cloud.requests().await.is_empty());
  assert!(backend.requests.lock().await[0].tools.is_empty());
  let runs = pair.worker_runs(&task);
  assert_eq!(runs[0].status, "completed");
  let events = pair.worker.store.events(&runs[0].id, 0, 1000).unwrap();
  let model: serde_json::Value = serde_json::from_str(
    &events
      .iter()
      .find(|event| event.kind == "model_selected")
      .unwrap()
      .payload_json,
  )
  .unwrap();
  assert_eq!(model["id"], "worker-fixture");
  let plan: serde_json::Value = serde_json::from_str(
    &events
      .iter()
      .find(|event| event.kind == "execution_plan")
      .unwrap()
      .payload_json,
  )
  .unwrap();
  assert_eq!(plan["allowed_tools"], json!([]));
  pair.stop().await;
}

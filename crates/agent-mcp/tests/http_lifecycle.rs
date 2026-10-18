mod support;
use agent_mcp::{McpLimits, McpManager};
use agent_runtime::{RuntimeError, RuntimeTool};
use serde_json::json;
use std::{
  sync::{Arc, atomic::Ordering},
  time::Duration,
};
use support::{Fixture, approval, call, server};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn timeout_cancels_protocol_request_and_reports_uncertain_without_replay() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), false).await;
  let limits = McpLimits {
    call_timeout: Duration::from_millis(200),
    ..Default::default()
  };
  let manager = McpManager::connect(vec![config], approval(true), limits)
    .await
    .expect("connect");
  let result = manager
    .execute(call(json!("wait")), CancellationToken::new())
    .await;
  assert!(matches!(result, Err(RuntimeError::Uncertain(_))));
  tokio::time::timeout(Duration::from_secs(2), fixture.cancelled.notified())
    .await
    .expect("protocol cancellation reached server");
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
  manager.shutdown().await;
  stop.cancel();
}

#[tokio::test]
async fn cancellation_after_dispatch_is_uncertain_and_sends_protocol_cancellation() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), false).await;
  let manager = Arc::new(
    McpManager::connect(vec![config], approval(true), McpLimits::default())
      .await
      .expect("connect"),
  );
  let cancellation = CancellationToken::new();
  let running = manager.clone();
  let signal = cancellation.clone();
  let task = tokio::spawn(async move { running.execute(call(json!("wait")), signal).await });
  tokio::time::timeout(Duration::from_secs(2), fixture.started.notified())
    .await
    .expect("actual dispatch");
  cancellation.cancel();
  assert!(matches!(
    task.await.expect("call task"),
    Err(RuntimeError::Uncertain(_))
  ));
  tokio::time::timeout(Duration::from_secs(2), fixture.cancelled.notified())
    .await
    .expect("server cancelled");
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
  Arc::try_unwrap(manager)
    .ok()
    .expect("unique manager")
    .shutdown()
    .await;
  stop.cancel();
}

#[tokio::test]
async fn daemon_connection_shutdown_fences_wrappers_and_pending_calls() {
  let fixture = Fixture::default();
  let (config, stop) = server(fixture.clone(), false).await;
  let approval = approval(true);
  let manager = Arc::new(
    McpManager::connect(vec![config], approval.clone(), McpLimits::default())
      .await
      .expect("connect"),
  );
  let tool = manager.tool_for(approval.clone());
  let running = tool.clone();
  let task = tokio::spawn(async move {
    running
      .execute(call(json!("wait")), CancellationToken::new())
      .await
  });
  tokio::time::timeout(Duration::from_secs(2), fixture.started.notified())
    .await
    .expect("actual dispatch");
  manager.cancel_connections();
  let pending = tokio::time::timeout(Duration::from_secs(2), task)
    .await
    .expect("pending call resolved")
    .expect("call task");
  assert!(matches!(pending, Err(RuntimeError::Uncertain(_))));
  assert!(matches!(
    tool
      .execute(call(json!("after shutdown")), CancellationToken::new())
      .await,
    Err(RuntimeError::Tool(_))
  ));
  assert_eq!(approval.seen.lock().await.len(), 1);
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
  drop(tool);
  Arc::try_unwrap(manager)
    .ok()
    .expect("unique manager")
    .shutdown()
    .await;
  stop.cancel();
}

#[path = "support/browser.rs"]
mod browser;
#[allow(dead_code)]
mod support;
use agent_protocol::v1;
use agent_test_support::{MockModelServer, StandardProtocol};
use browser::MockRuntime;
use std::sync::atomic::Ordering;
use support::TestNode;
async fn node() -> TestNode {
  let main = MockModelServer::start(vec![]).await.unwrap();
  let classifier = MockModelServer::start(vec![]).await.unwrap();
  TestNode::start(&main, &classifier, StandardProtocol::ChatCompletions).await
}
async fn open(node: &TestNode, host: &str) -> (v1::BrowserSpace, v1::BrowserTab) {
  let space = node
    .client
    .open_browser_space_on_host(&node.session_id, "fixture", false, host)
    .await
    .unwrap();
  let tab = node
    .client
    .open_browser_tab(&space.id, "about:blank")
    .await
    .unwrap();
  (space, tab)
}
fn action(tab: &v1::BrowserTab, id: &str) -> v1::BrowserAction {
  v1::BrowserAction {
    action_id: id.into(),
    tab_id: tab.id.clone(),
    snapshot_id: "snapshot-1".into(),
    revision: 1,
    kind: "click".into(),
    element_ref: "button".into(),
    text: String::new(),
    timeout_ms: 1000,
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_reverse_channel_routes_frames_screenshots_and_takeover() {
  let node = node().await;
  let runtime = MockRuntime::attach(&node.client, false, true).await;
  let (space, tab) = open(&node, &runtime.host_id).await;
  let frame = node.client.browser_frame(&tab.id, 0).await.unwrap();
  assert!(
    frame.bgra8.len() > 4 * 1024 * 1024,
    "must cross tonic's default4MiB limit"
  );
  assert_eq!(
    frame.bgra8.len(),
    frame.stride as usize * frame.height as usize
  );
  let snapshot = node.client.observe_browser(&tab.id, true).await.unwrap();
  assert!(!snapshot.screenshot_artifact.is_empty());
  let artifacts = agent_workspace::ArtifactStore::open(
    &node.daemon.config.data_path().unwrap().join("artifacts"),
    &node.daemon.node_id,
  )
  .unwrap();
  let png = artifacts.read(&snapshot.screenshot_artifact).unwrap();
  assert_eq!(
    image::load_from_memory(&png)
      .unwrap()
      .to_rgba8()
      .get_pixel(0, 0)
      .0,
    [255, 255, 255, 255]
  );
  let input = v1::BrowserInput {
    tab_id: tab.id.clone(),
    revision: 1,
    kind: "text".into(),
    text: "synthetic-human-password-F876C".into(),
    ..Default::default()
  };
  assert!(node.client.browser_input(input.clone()).await.is_err());
  assert_eq!(runtime.calls.load(Ordering::SeqCst), 0);
  node.client.browser_takeover(&space.id, true).await.unwrap();
  assert!(
    node
      .client
      .browser_action(action(&tab, &agent_core::new_id()))
      .await
      .is_err()
  );
  node.client.browser_input(input.clone()).await.unwrap();
  assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
  for filename in ["agent.sqlite3", "agent.sqlite3-wal"] {
    let path = node.daemon.config.data_path().unwrap().join(filename);
    if path.exists() {
      let bytes = std::fs::read(path).unwrap();
      assert!(
        !bytes
          .windows(input.text.len())
          .any(|part| part == input.text.as_bytes()),
        "human input must not persist in browser receipts, events or outbox"
      );
    }
  }
  node
    .client
    .browser_takeover(&space.id, false)
    .await
    .unwrap();
  assert!(node.client.browser_input(input).await.is_err());
  let id = agent_core::new_id();
  node.client.browser_action(action(&tab, &id)).await.unwrap();
  assert_eq!(
    node
      .daemon
      .store
      .browser_action_status(&id)
      .unwrap()
      .as_deref(),
    Some("completed")
  );
  assert!(node.client.browser_action(action(&tab, &id)).await.is_err());
  assert_eq!(runtime.calls.load(Ordering::SeqCst), 2);
  node.client.close_browser_space(&space.id).await.unwrap();
  assert!(node.client.browser_spaces().await.unwrap().is_empty());
  drop(runtime);
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_browser_action_is_uncertain_and_not_replayed_after_reconnect() {
  let node = node().await;
  let runtime = MockRuntime::attach(&node.client, true, false).await;
  let (_, tab) = open(&node, &runtime.host_id).await;
  let id = agent_core::new_id();
  let result = node.client.browser_action(action(&tab, &id)).await;
  assert!(result.is_err());
  assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
  assert_eq!(
    node
      .daemon
      .store
      .browser_action_status(&id)
      .unwrap()
      .as_deref(),
    Some("uncertain")
  );
  let replacement = MockRuntime::attach(&node.client, false, false).await;
  let (_, tab) = open(&node, &replacement.host_id).await;
  assert!(node.client.browser_action(action(&tab, &id)).await.is_err());
  assert_eq!(replacement.calls.load(Ordering::SeqCst), 0);
  assert_eq!(
    node
      .daemon
      .store
      .browser_action_status(&id)
      .unwrap()
      .as_deref(),
    Some("uncertain")
  );
  drop(runtime);
  drop(replacement);
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_runtime_hosts_require_explicit_selection_and_cross_session_ids_are_checked() {
  let node = node().await;
  let first = MockRuntime::attach(&node.client, false, false).await;
  let second = MockRuntime::attach(&node.client, false, false).await;
  assert!(
    node
      .client
      .open_browser_space(&node.session_id, "fixture", false)
      .await
      .is_err()
  );
  assert!(
    node
      .client
      .open_browser_space_on_host("missing-session", "fixture", false, &first.host_id)
      .await
      .is_err()
  );
  assert!(
    node
      .client
      .open_browser_space_on_host(&node.session_id, "../escape", false, &first.host_id)
      .await
      .is_err()
  );
  let (space, _) = open(&node, &first.host_id).await;
  assert_eq!(space.host_id, first.host_id);
  drop(first);
  drop(second);
  node.close().await;
}

//! Native regression: real CEF effects must remain uncertain after AX failure.
#[allow(dead_code)]
#[path = "native_browser_support/mod.rs"]
mod native_browser_support;
#[path = "native_browser_support/poison_page.rs"]
mod poison_page;
#[path = "browser_tool_support/receipts.rs"]
mod receipts;
#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use agent_browser::Command;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply_with_id};
use native_browser_support::{NativeRuntime, approval, model_observation, semantic};
use poison_page::PoisonPage;
use serde_json::{Value, json};
use std::time::Duration;
use support::TestNode;

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
#[ignore = "requires a real bundled native-cef acceptance executable"]
async fn responses_native_poison_click_stops_after_one_effect_with_uncertain_receipt() {
  let executable = std::path::PathBuf::from(
    std::env::var_os("LYONBROWN_NATIVE_CLIENT").expect("explicit native acceptance client"),
  );
  let protocol = StandardProtocol::Responses;
  let route = json!({"task_kind":"browser","model_policy":"general",
    "allowed_tools":["browser_observe","browser_act"],"execution_mode":"local",
    "target_hint":null,"risk_level":"high","confidence":0.99})
  .to_string();
  let classifier = MockModelServer::start(vec![text_reply(protocol, &route)])
    .await
    .unwrap();
  let model = MockModelServer::start_interactive().await.unwrap();
  let node = TestNode::start_configured(&model, &classifier, protocol, |config| {
    config.run_timeout_secs = 90;
  })
  .await;
  let runtime = NativeRuntime::attach(&node.client, &executable, false).await;
  let page = PoisonPage::start("Poison once").await;
  let space = node
    .client
    .open_browser_space_on_host(
      &node.session_id,
      "synthetic-poison-click",
      true,
      &runtime.host_id,
    )
    .await
    .unwrap();
  let tab = node
    .client
    .open_browser_tab(&space.id, &page.url)
    .await
    .unwrap();
  runtime.ready(&tab.id, "Poison once").await;
  let run = node.run("Click Poison once exactly once").await;
  model
    .push_reply(tool_reply_with_id(
      protocol,
      "observe-poison",
      "browser_observe",
      json!({"tab_id":tab.id,"screenshot":false}),
    ))
    .await;
  let observed = model_observation(&model, 2).await;
  let click = semantic(&observed, "click", "Poison once", "");
  model
    .push_reply(tool_reply_with_id(
      protocol,
      "click-poison",
      "browser_act",
      click.clone(),
    ))
    .await;
  let pending = approval(&node, &run.id, 0).await;
  let exact: Value = serde_json::from_str(&pending.arguments_json).unwrap();
  assert_eq!(exact["call"], click["call"]);
  assert_eq!(exact["session_id"], node.session_id);
  assert_eq!(exact["owner_epoch"], 1);
  assert_eq!(exact["host_id"], runtime.host_id);
  assert!(!exact["action_id"].as_str().unwrap().is_empty());
  assert!(runtime.actions.lock().await.is_empty());
  assert_eq!(page.count(), 0, "no HTTP effect before exact approval");
  // Frozen hosts incorrectly request this response and complete the Run. Fixed
  // hosts must terminate uncertain without consuming another model response.
  model
    .push_reply(text_reply(protocol, "Synthetic continuation detected"))
    .await;
  node.client.approve(&pending.id, true).await.unwrap();
  assert!(page.wait_effect().await, "one actual synthetic HTTP effect");
  let result = node.terminal(&run.id).await;
  let requests = model.requests().await;
  let actions = runtime.actions.lock().await.clone();
  let action_status = node
    .daemon
    .store
    .browser_action_status(exact["action_id"].as_str().unwrap())
    .unwrap();
  eprintln!(
    "synthetic poison click: terminal={}, requests={}, action_status={action_status:?}, actions={}, effects={}",
    result.status,
    requests.len(),
    actions.len(),
    page.count()
  );
  let recovered = node.daemon.recover().await;
  // Do not launch another Run on the known-bad host: its failure is already
  // observable, and the queued harmless final reply has been consumed.
  let fenced = if result.status == "uncertain" {
    Some(
      node
        .client
        .start_run(
          &node.session_id,
          "do not replay the synthetic click",
          &uuid::Uuid::now_v7().to_string(),
        )
        .await
        .is_err(),
    )
  } else {
    None
  };
  let recovered_requests = model.requests().await.len();
  let recovered_actions = runtime.actions.lock().await.len();
  let recovered_effects = page.count();
  // Clean up the real child before assertions that intentionally fail on V10.
  let shutdown = runtime.runtime.request(Command::Shutdown).await;
  drop(runtime);
  drop(page);
  assert!(shutdown.is_ok(), "native fixture shutdown: {shutdown:?}");
  assert_eq!(result.status, "uncertain", "{}", result.error);
  assert_eq!(requests.len(), 2, "uncertainty stops the sole Harness loop");
  assert!(
    requests
      .iter()
      .all(|request| request.path == protocol.path())
  );
  assert_eq!(actions.len(), 1, "one actual native action");
  assert_eq!(actions[0].id, exact["action_id"].as_str().unwrap());
  assert_eq!(action_status.as_deref(), Some("uncertain"));
  receipts::assert_receipts(&node, &pending, "consumed", Some("uncertain"));
  assert!(
    recovered.is_ok(),
    "recovery must preserve uncertain outcome"
  );
  assert_eq!(fenced, Some(true), "unresolved session fences new Runs");
  assert_eq!(recovered_requests, 2, "recovery sends no model request");
  assert_eq!(
    recovered_actions, 1,
    "recovery never replays the native action"
  );
  assert_eq!(recovered_effects, 1, "exactly one loopback HTTP effect");
  node.close().await;
}

async fn wait_requests(model: &MockModelServer, count: usize) {
  tokio::time::timeout(Duration::from_secs(30), model.wait_for_requests(count))
    .await
    .expect("Harness model request");
}

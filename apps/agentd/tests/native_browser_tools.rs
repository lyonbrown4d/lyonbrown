//! Opt-in acceptance across model protocols, the sole Harness, and actual CEF.
mod native_browser_support;
#[allow(dead_code)]
mod support;

use agent_browser::Command;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply_with_id};
use native_browser_support::{NativeRuntime, approval, effect_page, model_observation, semantic};
use serde_json::{Value, json};
use std::time::Duration;
use support::TestNode;

fn route() -> String {
  json!({"task_kind":"browser","model_policy":"general","allowed_tools":["browser_observe","browser_act"],
    "execution_mode":"local","target_hint":null,"risk_level":"high","confidence":0.99}).to_string()
}

/// LYONBROWN_NATIVE_CLIENT must name the bundled debug native acceptance client.
/// All endpoints and page effects stay on loopback; profiles contain synthetic data.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
#[ignore = "requires a real bundled native-cef acceptance executable"]
async fn all_protocols_harness_approve_real_cef_fill_click_and_fence_lost_receipts() {
  let executable = std::path::PathBuf::from(
    std::env::var_os("LYONBROWN_NATIVE_CLIENT").expect("explicit native acceptance client"),
  );
  for protocol in StandardProtocol::ALL {
    for lose_receipt in [false, true] {
      let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
        .await
        .unwrap();
      let model = MockModelServer::start_interactive().await.unwrap();
      let node = TestNode::start_configured(&model, &classifier, protocol, |config| {
        config.run_timeout_secs = 90;
      })
      .await;
      let runtime = NativeRuntime::attach(&node.client, &executable, lose_receipt).await;
      let page = effect_page().await;
      let space = node
        .client
        .open_browser_space_on_host(
          &node.session_id,
          "synthetic-harness",
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
      runtime.ready(&tab.id, "Submit native form").await;
      let run = node
        .run("Fill Name with Harness native value and submit the local form")
        .await;
      model
        .push_reply(tool_reply_with_id(
          protocol,
          "observe-native",
          "browser_observe",
          json!({"tab_id":tab.id,"screenshot":true}),
        ))
        .await;
      let observed = model_observation(&model, 2).await;
      assert!(!observed["screenshot_artifact"].as_str().unwrap().is_empty());
      let fill = semantic(&observed, "fill", "Name", "Harness native value");
      model
        .push_reply(tool_reply_with_id(
          protocol,
          "fill-native",
          "browser_act",
          fill.clone(),
        ))
        .await;
      let pending = approval(&node, &run.id, 0).await;
      let exact: Value = serde_json::from_str(&pending.arguments_json).unwrap();
      assert_eq!(exact["call"], fill["call"]);
      assert_eq!(exact["session_id"], node.session_id);
      assert_eq!(exact["owner_epoch"], 1);
      assert_eq!(exact["host_id"], runtime.host_id);
      assert!(
        runtime.actions.lock().await.is_empty(),
        "no CEF mutation before approval"
      );
      assert_eq!(page.effects.lock().await.len(), 0);
      node.client.approve(&pending.id, true).await.unwrap();
      let filled = model_observation(&model, 3).await;
      assert_eq!(
        runtime.actions.lock().await[0].id,
        exact["action_id"].as_str().unwrap()
      );
      assert!(
        filled["accessibility_json"]
          .as_str()
          .unwrap()
          .contains("Harness native value")
      );
      assert_eq!(
        node
          .daemon
          .store
          .browser_action_status(exact["action_id"].as_str().unwrap())
          .unwrap()
          .as_deref(),
        Some("completed")
      );
      let click = semantic(&filled, "click", "Submit native form", "");
      model
        .push_reply(tool_reply_with_id(
          protocol,
          "click-native",
          "browser_act",
          click.clone(),
        ))
        .await;
      let pending = approval(&node, &run.id, 1).await;
      let exact: Value = serde_json::from_str(&pending.arguments_json).unwrap();
      assert_eq!(exact["call"], click["call"]);
      assert_eq!(runtime.actions.lock().await.len(), 1);
      assert!(
        page.effects.lock().await.is_empty(),
        "form has no effect before click approval"
      );
      node.client.approve(&pending.id, true).await.unwrap();
      page.wait_effect().await;
      assert_eq!(
        runtime.actions.lock().await[1].id,
        exact["action_id"].as_str().unwrap()
      );
      assert_eq!(&*page.effects.lock().await, &["Harness native value"]);
      if lose_receipt {
        runtime.wait_lost_receipt().await;
        node.client.cancel_run(&run.id).await.unwrap();
        let result = node.terminal(&run.id).await;
        assert_eq!(result.status, "uncertain", "{}", result.error);
        assert_eq!(
          model.requests().await.len(),
          3,
          "unknown result stops Harness model loop"
        );
        assert_eq!(
          node
            .daemon
            .store
            .browser_action_status(exact["action_id"].as_str().unwrap())
            .unwrap()
            .as_deref(),
          Some("uncertain")
        );
        assert!(
          node
            .client
            .start_run(
              &node.session_id,
              "do not replay",
              &uuid::Uuid::now_v7().to_string()
            )
            .await
            .is_err()
        );
        node.daemon.recover().await.unwrap();
        assert_eq!(runtime.actions.lock().await.len(), 2);
        assert_eq!(
          page.effects.lock().await.len(),
          1,
          "recovery never repeats the native effect"
        );
      } else {
        let clicked = model_observation(&model, 4).await;
        assert!(
          clicked["accessibility_json"]
            .as_str()
            .unwrap()
            .contains("Submitted")
        );
        model
          .push_reply(text_reply(
            protocol,
            "Native form submitted after exact approval",
          ))
          .await;
        let result = node.terminal(&run.id).await;
        assert_eq!(result.status, "completed", "{}", result.error);
        assert!(result.result.contains("Native form submitted"));
        assert_eq!(
          node
            .daemon
            .store
            .browser_action_status(exact["action_id"].as_str().unwrap())
            .unwrap()
            .as_deref(),
          Some("completed")
        );
        assert!(
          model
            .requests()
            .await
            .iter()
            .all(|request| request.path == protocol.path())
        );
        assert_eq!(runtime.actions.lock().await.len(), 2);
      }
      runtime.ready(&tab.id, "Submitted").await;
      eprintln!(
        "{protocol:?}: actual CEF fill + click, exact approvals, page effect and durable {} verified",
        if lose_receipt {
          "uncertain/no-replay"
        } else {
          "completed receipt/output"
        }
      );
      runtime.runtime.request(Command::Shutdown).await.unwrap();
      drop(runtime);
      page.server.abort();
      node.close().await;
    }
  }
}

async fn wait_requests(model: &MockModelServer, count: usize) {
  tokio::time::timeout(Duration::from_secs(30), model.wait_for_requests(count))
    .await
    .expect("Harness model request");
}

//! Candidate regression: real CEF effects must remain uncertain after AX failure.
#[allow(dead_code)]
#[path = "native_browser_support/mod.rs"]
mod native_browser_support;
#[path = "browser_tool_support/receipts.rs"]
mod receipts;
#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use agent_browser::Command;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply_with_id};
use native_browser_support::{NativeRuntime, approval, model_observation, semantic};
use serde_json::{Value, json};
use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};
use support::TestNode;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  sync::Notify,
  task::{JoinHandle, JoinSet},
};

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
  let page = PoisonPage::start().await;
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
  page.wait_effect().await;
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

struct PoisonPage {
  url: String,
  effects: Arc<AtomicUsize>,
  changed: Arc<Notify>,
  server: JoinHandle<()>,
}

impl PoisonPage {
  async fn start() -> Self {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let effects = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(Notify::new());
    let recorded = effects.clone();
    let notify = changed.clone();
    let html = Arc::new(format!(
      r#"<!doctype html><meta charset="utf-8"><title>Synthetic poison click</title><button onclick="fetch('/effect',{{method:'POST',keepalive:true}});this.setAttribute('aria-label','x'.repeat({}))">Poison once</button>"#,
      agent_browser::MAX_AX_BYTES + 1024
    ));
    let server = tokio::spawn(async move {
      let mut clients = JoinSet::new();
      while let Ok((mut socket, _)) = listener.accept().await {
        while clients.try_join_next().is_some() {}
        let recorded = recorded.clone();
        let notify = notify.clone();
        let html = html.clone();
        clients.spawn(async move {
          let request = tokio::time::timeout(Duration::from_secs(2), async {
            let mut bytes = Vec::new();
            let mut chunk = [0; 1024];
            while bytes.len() < 8192 {
              let count = socket.read(&mut chunk).await.ok()?;
              if count == 0 {
                return None;
              }
              bytes.extend_from_slice(&chunk[..count]);
              if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                return Some(bytes);
              }
            }
            None
          })
          .await;
          let Ok(Some(bytes)) = request else { return };
          let request = String::from_utf8_lossy(&bytes);
          let first = request.lines().next().unwrap_or("");
          let body = if first.starts_with("POST /effect ") {
            recorded.fetch_add(1, Ordering::SeqCst);
            notify.notify_waiters();
            "ok"
          } else if first.starts_with("GET / ") {
            html.as_str()
          } else {
            ""
          };
          let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html;charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
          );
          let _ = tokio::time::timeout(
            Duration::from_secs(2),
            socket.write_all(response.as_bytes()),
          )
          .await;
        });
      }
    });
    Self {
      url,
      effects,
      changed,
      server,
    }
  }

  fn count(&self) -> usize {
    self.effects.load(Ordering::SeqCst)
  }

  async fn wait_effect(&self) {
    tokio::time::timeout(Duration::from_secs(20), async {
      loop {
        let changed = self.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if self.count() > 0 {
          return;
        }
        changed.await;
      }
    })
    .await
    .expect("one actual synthetic HTTP effect");
  }
}

impl Drop for PoisonPage {
  fn drop(&mut self) {
    self.server.abort();
  }
}

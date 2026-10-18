use crate::{support::TestNode, wait_requests};
use agent_browser::{Action, BrowserError, BrowserHost, Command, Reply, wire};
use agent_client::AgentClient;
use agent_protocol::v1;
use agent_test_support::MockModelServer;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  sync::{Mutex, Notify, mpsc},
  task::JoinHandle,
};
use tokio_stream::wrappers::ReceiverStream;

pub struct NativeRuntime {
  pub host_id: String,
  pub runtime: BrowserHost,
  pub actions: Arc<Mutex<Vec<Action>>>,
  lost: Arc<Notify>,
  task: JoinHandle<()>,
  _profiles: std::path::PathBuf,
}
impl NativeRuntime {
  pub async fn attach(client: &AgentClient, executable: &Path, lose_click_receipt: bool) -> Self {
    let profiles = tempfile::tempdir().unwrap().keep();
    eprintln!(
      "Preserved synthetic CEF acceptance profile: {}",
      profiles.display()
    );
    let runtime = BrowserHost::spawn(executable, &profiles).await.unwrap();
    let host_id = uuid::Uuid::now_v7().to_string();
    let (sender, receiver) = mpsc::channel(2);
    sender
      .send(v1::BrowserRuntimeResponse {
        host_id: host_id.clone(),
        protocol_version: agent_browser::RUNTIME_PROTOCOL_VERSION,
        request_id: String::new(),
        payload: vec![],
      })
      .await
      .unwrap();
    let mut requests = client
      .attach_browser_runtime(ReceiverStream::new(receiver))
      .await
      .unwrap();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let recorded = actions.clone();
    let lost = Arc::new(Notify::new());
    let notify = lost.clone();
    let native = runtime.clone();
    let host = host_id.clone();
    let task = tokio::spawn(async move {
      while let Some(request) = requests.message().await.unwrap() {
        let command: Command = wire::decode(&request.payload).unwrap();
        let lose =
          matches!(&command, Command::Act(action) if lose_click_receipt && action.kind == "click");
        if let Command::Act(action) = &command {
          recorded.lock().await.push(action.clone());
        }
        // Every request executes in the real isolated CEF process. Fault injection
        // loses a completed click's receipt, never substitutes a synthetic reply.
        let result = native.request(command).await;
        if lose {
          assert!(
            result.is_ok(),
            "native click must complete before losing its receipt: {result:?}"
          );
          notify.notify_one();
          std::future::pending::<()>().await;
        }
        sender
          .send(v1::BrowserRuntimeResponse {
            host_id: host.clone(),
            protocol_version: agent_browser::RUNTIME_PROTOCOL_VERSION,
            request_id: request.request_id,
            payload: wire::encode(&result).unwrap(),
          })
          .await
          .unwrap();
      }
    });
    Self {
      host_id,
      runtime,
      actions,
      lost,
      task,
      _profiles: profiles,
    }
  }
  pub async fn wait_lost_receipt(&self) {
    tokio::time::timeout(Duration::from_secs(20), self.lost.notified())
      .await
      .unwrap();
  }
  pub async fn ready(&self, tab: &str, needle: &str) {
    tokio::time::timeout(Duration::from_secs(30), async {
      loop {
        match self
          .runtime
          .request(Command::Observe {
            tab_id: tab.into(),
            screenshot: true,
          })
          .await
        {
          Ok(Reply::Snapshot(snapshot))
            if snapshot.accessibility_json.contains(needle) && snapshot.frame.is_some() =>
          {
            return;
          }
          Ok(Reply::Snapshot(_)) | Err(BrowserError::StaleSnapshot) => {}
          reply => panic!("real CEF page observation failed: {reply:?}"),
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
      }
    })
    .await
    .expect("actual native CEF page ready");
  }
}
impl Drop for NativeRuntime {
  fn drop(&mut self) {
    self.task.abort();
  }
}

pub async fn approval(node: &TestNode, run: &str, previous: usize) -> v1::Approval {
  tokio::time::timeout(Duration::from_secs(20), async {
    let mut events = node.client.watch_run(run, 0).await.unwrap();
    loop {
      let approvals = node.client.list_approvals(run).await.unwrap();
      if approvals.len() > previous {
        return approvals
          .into_iter()
          .find(|approval| approval.status == "pending")
          .expect("pending exact approval");
      }
      assert!(
        events.message().await.unwrap().is_some(),
        "run ended before approval"
      );
    }
  })
  .await
  .expect("native tool approval")
}

fn observation(value: &Value) -> Option<Value> {
  match value {
    Value::String(text) => serde_json::from_str::<Value>(text)
      .ok()
      .filter(|value| value["untrusted_page_content"] == true && value["observation"].is_object())
      .map(|value| value["observation"].clone()),
    Value::Array(values) => values.iter().rev().find_map(observation),
    Value::Object(values) => values.values().rev().find_map(observation),
    _ => None,
  }
}
pub async fn model_observation(model: &MockModelServer, count: usize) -> Value {
  wait_requests(model, count).await;
  let requests = model.requests().await;
  observation(&requests[count - 1].body).unwrap_or_else(|| {
    panic!(
      "model request lacks real CEF result: {}",
      requests[count - 1].body
    )
  })
}
pub fn semantic(observed: &Value, op: &str, name: &str, text: &str) -> Value {
  let nodes: Vec<Value> =
    serde_json::from_str(observed["accessibility_json"].as_str().unwrap()).unwrap();
  let element = nodes
    .iter()
    .find(|node| {
      node["name"] == name && matches!(node["role"].as_str(), Some("textbox" | "button"))
    })
    .expect("native semantic element");
  json!({"call":{"op":op,"tab_id":observed["tab_id"],"snapshot_id":observed["snapshot_id"],
    "revision":observed["revision"],"element_ref":element["ref"],"text":text,"timeout_ms":5000}})
}

pub struct EffectPage {
  pub url: String,
  pub effects: Arc<Mutex<Vec<String>>>,
  changed: Arc<Notify>,
  pub server: JoinHandle<()>,
}
impl EffectPage {
  pub async fn wait_effect(&self) {
    tokio::time::timeout(Duration::from_secs(20), async {
      loop {
        let changed = self.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if !self.effects.lock().await.is_empty() {
          return;
        }
        changed.await;
      }
    })
    .await
    .expect("actual form HTTP effect");
  }
}
pub async fn effect_page() -> EffectPage {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let url = format!("http://{}/", listener.local_addr().unwrap());
  let effects = Arc::new(Mutex::new(Vec::new()));
  let recorded = effects.clone();
  let changed = Arc::new(Notify::new());
  let notify = changed.clone();
  let server = tokio::spawn(async move {
    while let Ok((mut socket, _)) = listener.accept().await {
      let recorded = recorded.clone();
      let notify = notify.clone();
      tokio::spawn(async move {
        let mut bytes = [0; 8192];
        let count = socket.read(&mut bytes).await.unwrap();
        let request = String::from_utf8_lossy(&bytes[..count]);
        let path = request.split_whitespace().nth(1).unwrap_or("/");
        let body = if path.starts_with("/effect?") {
          let url = url::Url::parse(&format!("http://127.0.0.1{path}")).unwrap();
          let value = url
            .query_pairs()
            .find(|(key, _)| key == "value")
            .unwrap()
            .1
            .into_owned();
          recorded.lock().await.push(value);
          notify.notify_waiters();
          "ok"
        } else {
          r#"<!doctype html><meta charset="utf-8"><title>Harness CEF acceptance</title><label>Name<input id="name" aria-label="Name"></label><button onclick="document.getElementById('state').textContent='Submitted';fetch('/effect?value='+encodeURIComponent(document.getElementById('name').value))">Submit native form</button><p id="state">Ready</p>"#
        };
        let response = format!(
          "HTTP/1.1 200 OK\r\nContent-Type: text/html;charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
          body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
      });
    }
  });
  EffectPage {
    url,
    effects,
    changed,
    server,
  }
}

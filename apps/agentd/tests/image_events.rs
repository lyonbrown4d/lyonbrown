#[path = "image_events/persistence.rs"]
mod persistence;
mod support;

use std::sync::{Arc, Mutex};

use agent_core::{Event, RunStatus};
use agent_runtime::{RunOutcome, Runtime, RuntimeConfig, RuntimeImage};
use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use serde_json::{Value, json};
use support::TestNode;

// Synthetic one-pixel PNG; the only model endpoint is the loopback fixture.
const PNG: &str =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
const PROMPT: &str = "Describe this synthetic image.";

fn image_blocks<'a>(value: &'a Value, found: &mut Vec<&'a Value>) {
  match value {
    Value::Object(fields) => {
      if fields.get("type").and_then(Value::as_str) == Some("image") {
        found.push(value);
      }
      for child in fields.values() {
        image_blocks(child, found);
      }
    }
    Value::Array(values) => {
      for child in values {
        image_blocks(child, found);
      }
    }
    _ => {}
  }
}

fn assert_image_events(events: &[Event]) {
  assert!(!serde_json::to_string(events).unwrap().contains(PNG));
  assert!(!format!("{events:?}").contains(PNG));
  assert!(events.iter().any(|event| event.kind == "run_started"));
  assert!(
    events
      .iter()
      .any(|event| event.kind == "model_response_committed")
  );
  assert!(events.iter().any(|event| event.kind == "run_finished"));
  for event in events {
    let payload: Value = serde_json::from_str(&event.payload_json).unwrap();
    let mut images = Vec::new();
    image_blocks(&payload, &mut images);
    assert!(
      images.is_empty(),
      "image content escaped into {}",
      event.kind
    );
    if matches!(
      event.kind.as_str(),
      "run_started" | "model_response_committed"
    ) {
      let fields = payload.as_object().unwrap();
      assert_eq!(
        fields.len(),
        1,
        "only the run identity belongs in this audit event"
      );
      assert!(fields["run_id"].as_str().is_some_and(|id| !id.is_empty()));
    }
  }
}

async fn assert_snapshot(node: &TestNode) {
  let bytes = node
    .daemon
    .store
    .load_snapshot(&node.session_id)
    .await
    .unwrap()
    .unwrap();
  let snapshot: Value = serde_json::from_slice(&bytes).unwrap();
  assert_eq!(snapshot["version"], 1);
  assert_eq!(snapshot["session_id"], node.session_id);
  let entries = [snapshot["context"]["pinned"].clone()];
  let originals = entries
    .iter()
    .filter(|entry| entry["role"] == "user" && entry.to_string().contains(PROMPT))
    .collect::<Vec<_>>();
  assert_eq!(originals.len(), 1, "image input was duplicated");
  let mut images = Vec::new();
  image_blocks(originals[0], &mut images);
  assert_eq!(images.len(), 1);
  assert_eq!(images[0]["media_type"], "png");
  assert_eq!(images[0]["data"], json!({"type":"base64", "value":PNG}));
  assert!(images[0].get("dataRedacted").is_none());
}

async fn watch_events(node: &TestNode, run_id: &str) -> Vec<Event> {
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    let mut stream = node.client.watch_run(run_id, 0).await.unwrap();
    let mut events = Vec::new();
    while let Some(event) = stream.message().await.unwrap() {
      assert!(!event.payload_json.contains(PNG));
      assert!(!format!("{event:?}").contains(PNG));
      events.push(Event {
        cursor: event.cursor,
        run_id: event.run_id,
        kind: event.kind,
        payload_json: event.payload_json,
        created_at_ms: event.created_at_ms,
      });
    }
    events
  })
  .await
  .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn image_snapshot_restores_without_replay_and_events_never_expose_content() {
  let protocol = StandardProtocol::ChatCompletions;
  let main = MockModelServer::start(vec![text_reply(protocol, "Image received.")])
    .await
    .unwrap();
  let classifier = MockModelServer::start(Vec::new()).await.unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  let session = node
    .daemon
    .store
    .session(&node.session_id)
    .await
    .unwrap()
    .unwrap();
  let config = RuntimeConfig {
    session_id: session.id.clone(),
    owner_epoch: session.owner_epoch,
    provider: protocol.provider().into(),
    model: "main-fixture".into(),
    api_key: "synthetic-main-secret".into(),
    base_url: Some(main.base_url().into()),
    system_prompt: None,
    context_window: 128_000,
    max_output_tokens: 4096,
    supports_images: true,
  };
  let runtime_events = Arc::new(Mutex::new(Vec::new()));
  // Do not start_run: that would schedule a second Agent Loop in the daemon.
  let (run, created) = node
    .daemon
    .store
    .accept_run(
      &session.id,
      session.owner_epoch,
      PROMPT,
      &agent_core::new_id(),
    )
    .await
    .unwrap();
  assert!(created);
  node
    .daemon
    .store
    .update_run_owned(&run.id, session.owner_epoch, RunStatus::Running, "", "")
    .await
    .unwrap();
  let capture = runtime_events.clone();
  let runtime = Runtime::open(
    config.clone(),
    Arc::new(persistence::Persistence(node.daemon.store.clone())),
    Vec::new(),
    Arc::new(move |event| capture.lock().unwrap().push(event)),
  )
  .await
  .unwrap();
  let result = runtime
    .execute_input(
      PROMPT,
      vec![RuntimeImage {
        mime_type: "image/png".into(),
        data_base64: PNG.into(),
      }],
    )
    .await
    .unwrap();
  assert_eq!(result.outcome, RunOutcome::Completed);
  assert_eq!(result.text, "Image received.");
  drop(runtime);
  let terminal = node
    .daemon
    .store
    .update_run_owned(
      &run.id,
      session.owner_epoch,
      RunStatus::Completed,
      &result.text,
      "",
    )
    .await
    .unwrap();
  assert_snapshot(&node).await;
  let events = node.daemon.store.events(&run.id, 0, 1000).await.unwrap();
  assert_eq!(watch_events(&node, &run.id).await, events);
  assert_image_events(&events);
  let outbox = node.daemon.store.pending_outbox(1000).await.unwrap();
  assert_eq!(outbox, events);
  assert!(!serde_json::to_string(&outbox).unwrap().contains(PNG));
  assert!(!format!("{outbox:?}").contains(PNG));
  let snapshot = node.daemon.store.load_snapshot(&session.id).await.unwrap();
  let listener_count = runtime_events.lock().unwrap().len();
  let capture = runtime_events.clone();
  let reopened = Runtime::open(
    config,
    Arc::new(persistence::Persistence(node.daemon.store.clone())),
    Vec::new(),
    Arc::new(move |event| capture.lock().unwrap().push(event)),
  )
  .await;
  let reopened = reopened.expect("Rig message snapshots must restore image content");
  drop(reopened);
  assert_eq!(
    node.daemon.store.load_snapshot(&session.id).await.unwrap(),
    snapshot
  );
  assert_eq!(
    node.daemon.store.events(&run.id, 0, 1000).await.unwrap(),
    events
  );
  assert_eq!(
    node.daemon.store.pending_outbox(1000).await.unwrap(),
    outbox
  );
  assert_eq!(
    node.daemon.store.run(&run.id).await.unwrap(),
    Some(terminal)
  );
  assert_eq!(
    node.daemon.store.session(&session.id).await.unwrap(),
    Some(session)
  );
  assert_eq!(watch_events(&node, &run.id).await, events);
  {
    let runtime_events = runtime_events.lock().unwrap();
    assert_eq!(runtime_events.len(), listener_count);
    assert!(
      !serde_json::to_string(&*runtime_events)
        .unwrap()
        .contains(PNG)
    );
    assert!(!format!("{runtime_events:?}").contains(PNG));
  }
  let requests = main.requests().await;
  assert_eq!(
    requests.len(),
    1,
    "opening a persisted session must not call the provider"
  );
  let images = requests[0].body["messages"]
    .as_array()
    .unwrap()
    .iter()
    .filter_map(|message| message["content"].as_array())
    .flatten()
    .filter(|block| block["type"] == "image_url")
    .collect::<Vec<_>>();
  assert_eq!(images.len(), 1);
  assert_eq!(
    images[0]["image_url"]["url"],
    format!("data:image/png;base64,{PNG}")
  );
  assert!(classifier.requests().await.is_empty());
  node.close().await;
}

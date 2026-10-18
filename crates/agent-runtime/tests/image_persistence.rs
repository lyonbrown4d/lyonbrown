#[allow(dead_code)]
mod support;

use std::sync::{Arc, Mutex};

use agent_runtime::{RunOutcome, Runtime, RuntimeError, RuntimeImage, SessionMutation};
use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use serde_json::{Value, json};

// Synthetic one-pixel PNG: no external image service or paid model is involved.
const PNG: &str =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
const PROMPT: &str = "Describe this synthetic fixture image.";

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

fn assert_projected_images(value: &Value, expected: usize) {
  let mut images = Vec::new();
  image_blocks(value, &mut images);
  assert_eq!(images.len(), expected, "missing projected image metadata");
  for image in images {
    assert_eq!(
      image,
      &json!({
        "type": "image",
        "mimeType": "image/png",
        "dataRedacted": true,
        "encodedBytes": PNG.len(),
      })
    );
  }
}

fn assert_private_mutations(mutations: &[SessionMutation]) {
  assert!(!serde_json::to_string(mutations).unwrap().contains(PNG));
  assert!(!format!("{mutations:?}").contains(PNG));
  let user_entry = mutations
    .iter()
    .find(|mutation| {
      mutation.kind == "entry_appended" && mutation.payload["entry"]["message"]["role"] == "user"
    })
    .expect("initial image user entry was persisted");
  assert_projected_images(&user_entry.payload["entry"]["message"], 1);
  let operation = mutations
    .iter()
    .find(|mutation| {
      mutation.kind == "record_appended"
        && mutation.payload["record"]["type"] == "operation_started"
        && mutation.payload["record"]["intent"]["kind"] == "run"
    })
    .expect("run execution intent was persisted");
  assert_projected_images(&operation.payload["record"]["intent"]["originalPrompt"], 1);
  assert_eq!(
    operation.payload["record"]["intent"]["initialMessages"],
    json!([])
  );
}

fn assert_snapshot_image(store: &support::Memory) {
  let bytes = store.snapshot.lock().unwrap().clone().unwrap();
  let snapshot: Value = serde_json::from_slice(&bytes).unwrap();
  let entries = snapshot["state"]["entries"].as_array().unwrap();
  let originals = entries
    .iter()
    .filter(|entry| entry["message"]["role"] == "user" && entry.to_string().contains(PROMPT))
    .collect::<Vec<_>>();
  assert_eq!(
    originals.len(),
    1,
    "snapshot duplicated the initial user message"
  );
  let mut images = Vec::new();
  image_blocks(&originals[0]["message"], &mut images);
  assert_eq!(images.len(), 1);
  assert_eq!(images[0]["data"], PNG);
  assert_eq!(images[0]["mimeType"], "image/png");
  assert!(images[0].get("dataRedacted").is_none());
}

fn assert_wire_image(protocol: StandardProtocol, body: &Value) {
  let (messages, image_type) = match protocol {
    StandardProtocol::ChatCompletions => (&body["messages"], "image_url"),
    StandardProtocol::Responses => (&body["input"], "input_image"),
    StandardProtocol::Anthropic => (&body["messages"], "image"),
  };
  let images = messages
    .as_array()
    .unwrap()
    .iter()
    .filter_map(|message| message["content"].as_array())
    .flatten()
    .filter(|block| block["type"] == image_type)
    .collect::<Vec<_>>();
  assert_eq!(images.len(), 1, "{protocol:?} lost or duplicated the image");
  let data_url = format!("data:image/png;base64,{PNG}");
  match protocol {
    StandardProtocol::ChatCompletions => {
      assert_eq!(images[0]["image_url"]["url"], data_url);
    }
    StandardProtocol::Responses => assert_eq!(images[0]["image_url"], data_url),
    StandardProtocol::Anthropic => {
      assert_eq!(images[0]["source"]["type"], "base64");
      assert_eq!(images[0]["source"]["media_type"], "image/png");
      assert_eq!(images[0]["source"]["data"], PNG);
    }
  }
}

async fn image_recovery_is_explicitly_unavailable(protocol: StandardProtocol) {
  let server = MockModelServer::start(vec![text_reply(protocol, "Initial image received.")])
    .await
    .unwrap();
  let mut config = support::config(server.base_url());
  config.provider = protocol.provider().into();
  let store = support::shared();
  let events = Arc::new(Mutex::new(Vec::new()));
  let capture = events.clone();
  let runtime = Runtime::open(
    config.clone(),
    store.clone(),
    Vec::new(),
    Arc::new(move |event| capture.lock().unwrap().push(event)),
  )
  .await
  .unwrap();
  let first = runtime
    .execute_input(
      PROMPT,
      vec![RuntimeImage {
        mime_type: "image/png".into(),
        data_base64: PNG.into(),
      }],
    )
    .await
    .unwrap();
  assert_eq!(first.outcome, RunOutcome::Completed, "{protocol:?}");
  assert_eq!(first.text, "Initial image received.");
  assert_snapshot_image(&store);
  assert_private_mutations(&store.mutations.lock().unwrap());
  {
    let captured = events.lock().unwrap();
    assert!(!serde_json::to_string(&*captured).unwrap().contains(PNG));
    assert!(!format!("{captured:?}").contains(PNG));
  }
  let requests = server.requests().await;
  assert_eq!(
    requests.len(),
    1,
    "{protocol:?} repeated the initial request"
  );
  assert_eq!(requests[0].path, protocol.path());
  assert_wire_image(protocol, &requests[0].body);
  let snapshot_before = store.snapshot.lock().unwrap().clone();
  let mutations_before = serde_json::to_value(&*store.mutations.lock().unwrap()).unwrap();
  let events_before = serde_json::to_value(&*events.lock().unwrap()).unwrap();
  drop(runtime);

  let capture = events.clone();
  let reopened = Runtime::open(
    config,
    store.clone(),
    Vec::new(),
    Arc::new(move |event| capture.lock().unwrap().push(event)),
  )
  .await;
  match reopened {
    Err(RuntimeError::Configuration(message)) => assert!(
      message.contains("image-bearing session recovery is unavailable"),
      "unexpected recovery capability error: {message}"
    ),
    Err(error) => panic!("expected an explicit capability error, got {error}"),
    Ok(_) => panic!("{protocol:?} silently accepted unsupported image recovery"),
  }
  assert_eq!(
    server.requests().await.len(),
    1,
    "recovery replayed a request"
  );
  assert_eq!(*store.snapshot.lock().unwrap(), snapshot_before);
  assert_eq!(
    serde_json::to_value(&*store.mutations.lock().unwrap()).unwrap(),
    mutations_before
  );
  assert_eq!(
    serde_json::to_value(&*events.lock().unwrap()).unwrap(),
    events_before
  );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_image_events_are_private_and_recovery_is_unavailable() {
  image_recovery_is_explicitly_unavailable(StandardProtocol::ChatCompletions).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_image_events_are_private_and_recovery_is_unavailable() {
  image_recovery_is_explicitly_unavailable(StandardProtocol::Responses).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anthropic_image_events_are_private_and_recovery_is_unavailable() {
  image_recovery_is_explicitly_unavailable(StandardProtocol::Anthropic).await;
}

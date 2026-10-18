//! Real CEF mouse-up must preserve uncertainty after an observable page effect.
#[allow(dead_code)]
#[path = "native_browser_support/mod.rs"]
mod native_browser_support;
#[path = "native_browser_support/poison_page.rs"]
mod poison_page;
#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use agent_browser::Command;
use agent_core::Event;
use agent_protocol::v1;
use agent_test_support::{MockModelServer, StandardProtocol};
use native_browser_support::NativeRuntime;
use poison_page::PoisonPage;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use support::TestNode;

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
#[ignore = "requires a real bundled native-cef acceptance executable"]
async fn human_mouse_up_effect_is_uncertain_and_never_replayed() {
  let executable = PathBuf::from(
    std::env::var_os("LYONBROWN_NATIVE_CLIENT").expect("explicit native acceptance client"),
  );
  let model = MockModelServer::start(vec![]).await.unwrap();
  let classifier = MockModelServer::start(vec![]).await.unwrap();
  let node = TestNode::start(&model, &classifier, StandardProtocol::Responses).await;
  let runtime = NativeRuntime::attach(&node.client, &executable, false).await;
  let page = PoisonPage::start("Poison human click").await;
  let space = node
    .client
    .open_browser_space_on_host(
      &node.session_id,
      "synthetic-human-input-poison",
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
  runtime.ready(&tab.id, "Poison human click").await;
  node.client.browser_takeover(&space.id, true).await.unwrap();
  let observed = fresh_observation(&node, &tab.id).await;
  let input = |kind: &str| v1::BrowserInput {
    tab_id: tab.id.clone(),
    revision: observed.revision,
    kind: kind.into(),
    x: 80,
    y: 45,
    button: 0,
    ..Default::default()
  };
  // Both halves intentionally use the same observed revision. Successful down
  // must not itself retire the frame revision required by its paired up.
  let down = node.client.browser_input(input("mouse_down")).await;
  let stream = format!("browser:{}", node.session_id);
  let store = &node.daemon.store;
  let cursor = store
    .events(&stream, 0, 1000)
    .unwrap()
    .last()
    .map_or(0, |event| event.cursor);
  let up = node.client.browser_input(input("mouse_up")).await;
  let code = up
    .as_ref()
    .err()
    .and_then(|error| error.downcast_ref::<tonic::Status>())
    .map(tonic::Status::code);
  let effect_seen = page.wait_effect().await;
  let events = store.events(&stream, cursor, 1000).unwrap();
  let intents: Vec<_> = events
    .iter()
    .filter(|event| event.kind == "browser_action_intent")
    .collect();
  let finished: Vec<_> = events
    .iter()
    .filter(|event| event.kind == "browser_action_finished")
    .collect();
  let action = intents
    .first()
    .and_then(|event| payload(event)["action_id"].as_str().map(str::to_owned));
  let status = action
    .as_ref()
    .and_then(|id| store.browser_action_status(id).unwrap());
  let outbox = store.pending_outbox(10_000).unwrap();
  let effects_before_recovery = page.count();
  let recovered = node.daemon.recover().await;
  let recovered_status = action
    .as_ref()
    .and_then(|id| store.browser_action_status(id).unwrap());
  let recovered_events = store.events(&stream, cursor, 1000).unwrap();
  let recovered_effects = page.count();
  let model_requests = model.requests().await.len();
  let classifier_requests = classifier.requests().await.len();
  eprintln!(
    "human poison: down={down:?}, up={up:?}, code={code:?}, effect_seen={effect_seen}, \
     effects={effects_before_recovery}/{recovered_effects}, status={status:?}/{recovered_status:?}, \
     events={events:?}, recover={recovered:?}, model={model_requests}, classifier={classifier_requests}"
  );
  // Always stop the real child before the regression assertions can fail.
  let shutdown = runtime.runtime.request(Command::Shutdown).await;
  drop(runtime);
  drop(page);
  node.close().await;

  assert!(shutdown.is_ok(), "native fixture shutdown: {shutdown:?}");
  assert!(down.is_ok(), "same-revision mouse-down: {down:?}");
  assert!(
    effect_seen,
    "the actual click must reach the loopback effect endpoint"
  );
  assert_eq!(effects_before_recovery, 1, "exactly one actual HTTP effect");
  assert_eq!(
    code,
    Some(tonic::Code::Unknown),
    "post-effect mouse-up: {up:?}"
  );
  assert_eq!(status.as_deref(), Some("uncertain"));
  assert_eq!(
    intents.len(),
    1,
    "one mouse-up intent after the pre-up cursor"
  );
  assert_eq!(finished.len(), 1, "one mouse-up terminal receipt");
  assert_eq!(events.len(), 2, "no extra mouse-up mutation or replay");
  let action = action.expect("mouse-up receipt identity");
  assert_eq!(
    payload(intents[0]),
    json!({"action_id":action,"kind":"browser_command"})
  );
  assert_eq!(
    payload(finished[0]),
    json!({"action_id":action,"status":"uncertain"})
  );
  assert!(intents[0].cursor < finished[0].cursor);
  for event in &events {
    let copies: Vec<_> = outbox
      .iter()
      .filter(|item| item.run_id == event.run_id && item.cursor == event.cursor)
      .collect();
    assert_eq!(
      copies,
      vec![event],
      "receipt must match its durable Outbox copy"
    );
  }
  assert!(recovered.is_ok(), "recovery: {recovered:?}");
  assert_eq!(recovered_status.as_deref(), Some("uncertain"));
  assert_eq!(
    recovered_events, events,
    "recovery must not issue another input intent"
  );
  assert_eq!(
    recovered_effects, 1,
    "recovery must not replay the human click"
  );
  assert_eq!(model_requests, 0, "human input starts no model Run");
  assert_eq!(
    classifier_requests, 0,
    "human input starts no classifier request"
  );
}

async fn fresh_observation(node: &TestNode, tab: &str) -> v1::BrowserObservation {
  tokio::time::timeout(Duration::from_secs(20), async {
    loop {
      match node.client.observe_browser(tab, false).await {
        Ok(observed) if observed.accessibility_json.contains("Poison human click") => {
          return observed;
        }
        Ok(_) => {}
        Err(error)
          if error
            .downcast_ref::<tonic::Status>()
            .is_some_and(|status| status.code() == tonic::Code::FailedPrecondition) => {}
        Err(error) => panic!("fresh human observation: {error:?}"),
      }
      tokio::time::sleep(Duration::from_millis(40)).await;
    }
  })
  .await
  .expect("fresh observed revision after explicit takeover")
}

fn payload(event: &Event) -> Value {
  serde_json::from_str(&event.payload_json).unwrap()
}

// Required by the reused NativeRuntime support module; this candidate invokes
// neither model_observation nor a model Run.
async fn wait_requests(model: &MockModelServer, count: usize) {
  tokio::time::timeout(Duration::from_secs(30), model.wait_for_requests(count))
    .await
    .expect("Harness model request");
}

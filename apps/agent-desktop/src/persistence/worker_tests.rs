use super::*;
use crate::local_state::TabState;
use gpui_kit::component::{ThemeMode, dock::DockAreaState};
use std::{sync::Arc, time::Duration};

#[test]
fn stop_ack_survives_a_full_presentation_queue() {
  let runtime = Arc::new(
    tokio::runtime::Builder::new_multi_thread()
      .worker_threads(2)
      .enable_all()
      .build()
      .expect("actor runtime"),
  );
  runtime.block_on(async {
    let directory = tempfile::tempdir().expect("private state directory");
    let (replies, receiver) = async_channel::bounded(1);
    replies
      .try_send(ScopedUpdate::RequestFinished)
      .expect("occupy bounded UI queue");
    let refill = replies.clone();
    let scope = "http://127.0.0.1:1\0node-a".to_owned();
    let sender = start(
      runtime.clone(),
      Arc::new(std::sync::Mutex::new(None)),
      ActorConfig {
        root: directory.path().into(),
        endpoint: "http://127.0.0.1:1".into(),
        node_id: "node-a".into(),
        scope: scope.clone(),
        generation: 1,
      },
      None,
      replies,
    );
    assert!(matches!(
      tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .expect("read queued fixture event")
        .expect("queue remains open"),
      ScopedUpdate::RequestFinished
    ));
    assert!(matches!(
      tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .expect("loaded reply should follow once the queue drains")
        .expect("queue remains open"),
      ScopedUpdate::Persistence(ResultMessage::Loaded { .. })
    ));
    refill
      .try_send(ScopedUpdate::RequestFinished)
      .expect("refill bounded UI queue before final flush");
    let state = DesktopState {
      tabs: vec![TabState {
        id: 1,
        session_id: None,
        unbound_from: None,
        composer: "unsent".into(),
        workspace: "/tmp/workspace".into(),
      }],
      active_tab: 1,
      show_sidebar: true,
      show_workbench: false,
      dock: DockAreaState::default(),
      layout_sizes: Vec::new(),
      theme: ThemeMode::Light,
      editor_drafts: Vec::new(),
    };
    let (command, completion) = Command::stop(scope, 1, 1, Some(state));
    sender.send(command).await.expect("send final flush");
    tokio::time::timeout(Duration::from_secs(3), completion)
      .await
      .expect("final flush must not wait for UI queue")
      .expect("actor sends store-release acknowledgement");
    drop(sender);
    let store = LocalStateStore::new(
      directory.path().into(),
      "desktop",
      "http://127.0.0.1:1",
      "node-a",
    )
    .expect("reopen released store");
    assert_eq!(
      store
        .load::<DesktopState>()
        .expect("load written state")
        .unwrap()
        .tabs[0]
        .composer,
      "unsent"
    );
  });
}

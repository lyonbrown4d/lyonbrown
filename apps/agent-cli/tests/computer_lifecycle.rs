#[path = "support/computer_client.rs"]
mod client;
#[path = "support/computer.rs"]
mod computer;
use client::{command, next, unregistered};
#[path = "support/node.rs"]
mod node;

use agent_cli::{
  backend::{self, Action, Update},
  computer::{ComputerError, ComputerStatus, LocalCommand},
  config::Config,
};
use agent_computer_runtime::DesktopScope;
use clap::Parser;
use std::{
  sync::{Arc, atomic::Ordering},
  time::Duration,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn explicit_selection_detach_reconnect_disconnect_and_exit_own_real_attachment() {
  let model =
    agent_test_support::MockModelServer::start(vec![agent_test_support::ScriptedReply::stall()])
      .await
      .unwrap();
  let mut node = node::Node::start(&model).await;
  let a = node
    .client
    .create_session("A", &node.workspace.path().to_string_lossy())
    .await
    .unwrap();
  let b = node
    .client
    .create_session("B", &node.workspace.path().to_string_lossy())
    .await
    .unwrap();
  let config = Config::try_parse_from([
    "agent-cli",
    "--endpoint",
    &node.endpoint(),
    "--token",
    node.daemon.auth_token(),
    "--data-dir",
    node.data.path().to_str().unwrap(),
  ])
  .unwrap()
  .resolve(true)
  .unwrap();
  let factory = Arc::new(computer::Factory::default());
  let (actions, mut updates) = backend::spawn_with_computer_factory(config, factory.clone());
  next(&mut updates, |update| {
    matches!(
      update,
      Update::Computer {
        status: ComputerStatus::Detached,
        ..
      }
    )
  })
  .await;
  actions.send(Action::Connect).await.unwrap();
  next(&mut updates, |update| {
    matches!(update, Update::Connected(_))
  })
  .await;
  actions
    .send(Action::Select {
      session: a.id.clone(),
      generation: 1,
    })
    .await
    .unwrap();
  next(&mut updates, |update| matches!(update, Update::Runs { .. })).await;
  let (status, detail) = command(&actions, &mut updates, &a.id, LocalCommand::Targets).await;
  assert_eq!(status, ComputerStatus::Detached);
  assert!(detail.contains("window 7"));
  assert!(factory.attachments.lock().unwrap().is_empty());
  assert!(
    node
      .daemon
      .computer_runtime_info(&a.id)
      .await
      .unwrap()
      .is_none()
  );

  factory.generation.fetch_add(1, Ordering::SeqCst);
  let (stale, _) = command(
    &actions,
    &mut updates,
    &a.id,
    LocalCommand::Attach(DesktopScope::Window(7)),
  )
  .await;
  assert!(matches!(stale, ComputerStatus::Stopped { .. }));
  assert_eq!(factory.selected.lock().unwrap()[0].process_id, Some(19));
  assert!(factory.attachments.lock().unwrap().is_empty());
  assert!(
    node
      .daemon
      .computer_runtime_info(&a.id)
      .await
      .unwrap()
      .is_none()
  );
  command(&actions, &mut updates, &a.id, LocalCommand::Targets).await;
  let (status, _) = command(
    &actions,
    &mut updates,
    &a.id,
    LocalCommand::Attach(DesktopScope::Window(7)),
  )
  .await;
  assert_eq!(
    status,
    ComputerStatus::Attached {
      session: a.id.clone(),
      scope: DesktopScope::Window(7)
    }
  );
  let info = node
    .daemon
    .computer_runtime_info(&a.id)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(info.desktop_session.scope, DesktopScope::Window(7));
  actions
    .send(Action::Select {
      session: b.id.clone(),
      generation: 2,
    })
    .await
    .unwrap();
  next(&mut updates, |update| matches!(update, Update::Runs { .. })).await;
  let (selected_status, _) = command(&actions, &mut updates, &b.id, LocalCommand::Status).await;
  assert_eq!(selected_status, status);
  let (rejected_status, detail) = command(
    &actions,
    &mut updates,
    &b.id,
    LocalCommand::Attach(DesktopScope::Display(2)),
  )
  .await;
  assert_eq!(rejected_status, status);
  assert_eq!(detail, ComputerError::AlreadyAttached.to_string());
  assert!(
    node
      .daemon
      .computer_runtime_info(&b.id)
      .await
      .unwrap()
      .is_none()
  );
  assert_eq!(
    command(&actions, &mut updates, &b.id, LocalCommand::Detach)
      .await
      .0,
    ComputerStatus::Detached
  );
  unregistered(&node, &a.id).await;
  let (_, detail) = command(
    &actions,
    &mut updates,
    &b.id,
    LocalCommand::Attach(DesktopScope::Display(99)),
  )
  .await;
  assert_eq!(detail, ComputerError::TargetMissing.to_string());
  assert_eq!(factory.attachments.lock().unwrap().len(), 1);

  command(
    &actions,
    &mut updates,
    &b.id,
    LocalCommand::Attach(DesktopScope::Display(2)),
  )
  .await;
  actions.send(Action::Connect).await.unwrap();
  next(&mut updates, |update| {
    matches!(update, Update::Connected(_))
  })
  .await;
  unregistered(&node, &b.id).await;
  assert_eq!(factory.attachments.lock().unwrap().len(), 2);
  let (status, _) = command(&actions, &mut updates, &b.id, LocalCommand::Status).await;
  assert!(matches!(status, ComputerStatus::Stopped { .. }));

  let (_, detail) = command(
    &actions,
    &mut updates,
    &a.id,
    LocalCommand::Attach(DesktopScope::Window(7)),
  )
  .await;
  assert_eq!(detail, ComputerError::CatalogRequired.to_string());
  command(&actions, &mut updates, &a.id, LocalCommand::Targets).await;
  command(
    &actions,
    &mut updates,
    &a.id,
    LocalCommand::Attach(DesktopScope::Window(7)),
  )
  .await;
  node.disconnect().await;
  next(&mut updates, |update| {
    matches!(
      update,
      Update::Computer {
        status: ComputerStatus::Stopped { .. },
        ..
      }
    )
  })
  .await;
  unregistered(&node, &a.id).await;
  node.reconnect().await;
  actions.send(Action::Connect).await.unwrap();
  next(&mut updates, |update| {
    matches!(update, Update::Connected(_))
  })
  .await;
  assert_eq!(factory.attachments.lock().unwrap().len(), 3);
  let (_, detail) = command(
    &actions,
    &mut updates,
    &a.id,
    LocalCommand::Attach(DesktopScope::Window(7)),
  )
  .await;
  assert_eq!(detail, ComputerError::CatalogRequired.to_string());
  command(&actions, &mut updates, &a.id, LocalCommand::Targets).await;
  command(
    &actions,
    &mut updates,
    &a.id,
    LocalCommand::Attach(DesktopScope::Window(7)),
  )
  .await;
  // A real daemon job survives client shutdown; local runtime commands create no runs.
  let run = node
    .client
    .start_run(
      &a.id,
      "keep daemon job running",
      &uuid::Uuid::now_v7().to_string(),
    )
    .await
    .unwrap();
  node.status(&run.id, "running").await;
  let (done, closed) = tokio::sync::oneshot::channel();
  actions.send(Action::Shutdown(done)).await.unwrap();
  closed.await.unwrap();
  unregistered(&node, &a.id).await;
  assert_eq!(
    factory.attachments.lock().unwrap().as_slice(),
    &[
      (a.id.clone(), DesktopScope::Window(7)),
      (b.id.clone(), DesktopScope::Display(2)),
      (a.id.clone(), DesktopScope::Window(7)),
      (a.id.clone(), DesktopScope::Window(7)),
    ]
  );
  tokio::time::timeout(Duration::from_secs(3), async {
    while factory.effects.drops.load(Ordering::SeqCst) != 4 {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  assert_eq!(factory.effects.captures.load(Ordering::SeqCst), 0);
  assert_eq!(factory.effects.inputs.load(Ordering::SeqCst), 0);
  let runs = node.run_count(&a.id, 1).await;
  assert_eq!(runs[0].id, run.id);
  assert_eq!(
    node.client.get_run(&run.id).await.unwrap().status,
    "running"
  );
  assert!(node.client.list_runs(&b.id).await.unwrap().is_empty());
  node.client.cancel_run(&run.id).await.unwrap();
  node.status(&run.id, "cancelled").await;
  node.close().await;
}

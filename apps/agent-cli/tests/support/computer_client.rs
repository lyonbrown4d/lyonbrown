use agent_cli::{
  backend::{Action, Update},
  computer::{ComputerStatus, LocalCommand},
};
use std::time::Duration;
use tokio::sync::mpsc;

pub async fn next(
  updates: &mut mpsc::Receiver<Update>,
  matches: impl Fn(&Update) -> bool,
) -> Update {
  tokio::time::timeout(Duration::from_secs(8), async {
    loop {
      let update = updates.recv().await.expect("backend alive");
      if matches(&update) {
        return update;
      }
    }
  })
  .await
  .expect("client update deadline")
}
pub async fn command(
  actions: &mpsc::Sender<Action>,
  updates: &mut mpsc::Receiver<Update>,
  session: &str,
  command: LocalCommand,
) -> (ComputerStatus, String) {
  actions
    .send(Action::Computer {
      session: Some(session.into()),
      command,
    })
    .await
    .unwrap();
  let Update::Computer { status, detail } = next(updates, |update| matches!(update, Update::Computer { status, .. } if !matches!(status, ComputerStatus::Attaching { .. }))).await else { unreachable!() };
  (status, detail)
}
pub async fn unregistered(node: &crate::node::Node, session: &str) {
  tokio::time::timeout(Duration::from_secs(3), async {
    while node
      .daemon
      .computer_runtime_info(session)
      .await
      .unwrap()
      .is_some()
    {
      tokio::task::yield_now().await;
    }
  })
  .await
  .expect("attachment removed from daemon");
}

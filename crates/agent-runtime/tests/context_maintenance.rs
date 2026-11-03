#[allow(dead_code)]
mod support;
use agent_runtime::{Runtime, SummaryInput, SummaryRequestGuard, cancel_unsent_summary};
use agent_test_support::{MockModelServer, ScriptedReply};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
struct Guard {
  entered: Arc<Notify>,
  hold: bool,
}
#[async_trait::async_trait]
impl SummaryRequestGuard for Guard {
  async fn prepare(
    &self,
    input: SummaryInput,
    _: CancellationToken,
  ) -> agent_runtime::Result<SummaryInput> {
    self.entered.notify_one();
    if self.hold {
      std::future::pending::<()>().await;
    }
    Ok(input)
  }
}
#[tokio::test]
async fn pure_recovery_clears_only_unsent_summary_intent_and_performs_no_model_io() {
  for sending in [false, true] {
    let server = MockModelServer::start(vec![
      support::text("old finding"),
      support::text("recent finding"),
      ScriptedReply::stall(),
    ])
    .await
    .expect("mock");
    let store = support::shared();
    let entered = Arc::new(Notify::new());
    let runtime = Arc::new(
      Runtime::open(
        support::config(server.base_url()),
        store.clone(),
        vec![],
        Arc::new(|_| {}),
      )
      .await
      .expect("runtime")
      .with_summary_guard(Arc::new(Guard {
        entered: entered.clone(),
        hold: !sending,
      })),
    );
    runtime
      .execute("original exact task")
      .await
      .expect("first task");
    runtime
      .execute("later exact task")
      .await
      .expect("second task");
    let worker = runtime.clone();
    let task = tokio::spawn(async move { worker.compact("preserve findings").await });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
      .await
      .expect("summary guard started");
    if sending {
      tokio::time::timeout(
        std::time::Duration::from_secs(5),
        server.wait_for_requests(3),
      )
      .await
      .expect("summary request sent");
    }
    task.abort();
    let _ = task.await;
    let before = store.snapshot.lock().expect("snapshot").clone();
    let result = cancel_unsent_summary("session", 1, store.clone()).await;
    if sending {
      assert!(result.is_err());
      assert_eq!(*store.snapshot.lock().expect("snapshot"), before);
      assert!(
        Runtime::open(
          support::config(server.base_url()),
          store.clone(),
          vec![],
          Arc::new(|_| {})
        )
        .await
        .is_err()
      );
    } else {
      assert!(result.expect("safe unsent cleanup"));
      let reopened = Runtime::open(
        support::config(server.base_url()),
        store.clone(),
        vec![],
        Arc::new(|_| {}),
      )
      .await
      .expect("reopen");
      assert!(
        !reopened
          .context_status()
          .await
          .expect("status")
          .summary_request_pending
      );
    }
    assert_eq!(server.requests().await.len(), if sending { 3 } else { 2 });
  }
}

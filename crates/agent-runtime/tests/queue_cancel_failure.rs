//! Upstream rpi-harness cancellation reproducer (reported on 0.14.0;
//! the warning-only failure path remains in 0.16.0). No model request is made.
//! Remove/replace this characterization once upstream propagates cancellation storage errors.
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use rpi_agent::message::AgentMessage;
use rpi_ai::UserMessage;
use rpi_harness::{
  agent_harness::{AgentHarness, AgentLane, CancelQueuedOutcome},
  error::{SessionError, SessionResult},
  session::{
    DefaultIdGenerator, Session,
    memory::{InMemorySessionStorage, SystemClock},
    types::*,
  },
  types::AgentHarnessOptions,
};

struct RefuseCancellation {
  inner: InMemorySessionStorage,
  failures: AtomicUsize,
}

impl RefuseCancellation {
  fn new() -> Self {
    Self {
      inner: InMemorySessionStorage::new(
        SessionMetadata {
          id: "queue-cancel-reproducer".into(),
          created_at: 0,
          parent_session_id: None,
        },
        Arc::new(SystemClock),
        Arc::new(DefaultIdGenerator::new()),
      ),
      failures: AtomicUsize::new(0),
    }
  }
}

#[async_trait::async_trait]
impl SessionStorage for RefuseCancellation {
  fn metadata(&self) -> SessionMetadata {
    self.inner.metadata()
  }
  async fn get_lanes(&self) -> SessionResult<Vec<LanePointer>> {
    self.inner.get_lanes().await
  }
  async fn create_lane(&self, lane: &str, at: Option<&str>) -> SessionResult<()> {
    self.inner.create_lane(lane, at).await
  }
  async fn move_lane(&self, lane: &str, to: Option<&str>) -> SessionResult<()> {
    self.inner.move_lane(lane, to).await
  }
  async fn append_entry(&self, entry: ProvisionedEntry, lane: &str) -> SessionResult<Entry> {
    self.inner.append_entry(entry, lane).await
  }
  async fn append_record(&self, record: LaneRecord) -> SessionResult<LaneRecord> {
    if matches!(record, LaneRecord::QueueCancelled(_)) {
      self.failures.fetch_add(1, Ordering::SeqCst);
      return Err(SessionError::storage(
        "synthetic cancellation storage failure",
      ));
    }
    self.inner.append_record(record).await
  }
  async fn get_entry(&self, id: &str) -> SessionResult<Option<Entry>> {
    self.inner.get_entry(id).await
  }
  async fn find_entries(&self, query: &EntryQuery) -> SessionResult<Vec<Entry>> {
    self.inner.find_entries(query).await
  }
  async fn find_entries_on_branch(
    &self,
    query: &EntryQuery,
    bounds: &BranchBounds,
    start: &str,
  ) -> SessionResult<Vec<Entry>> {
    self
      .inner
      .find_entries_on_branch(query, bounds, start)
      .await
  }
  async fn find_records(&self, query: &RecordQuery) -> SessionResult<Vec<LaneRecord>> {
    self.inner.find_records(query).await
  }
  async fn find_open_operations(
    &self,
    lane: &str,
    limit: Option<usize>,
  ) -> SessionResult<Vec<OperationStartedRecord>> {
    self.inner.find_open_operations(lane, limit).await
  }
  async fn get_log(&self, options: &LogOptions) -> SessionResult<Vec<LogItem>> {
    self.inner.get_log(options).await
  }
  async fn get_name(&self) -> SessionResult<Option<String>> {
    self.inner.get_name().await
  }
  async fn set_name(&self, name: Option<&str>) -> SessionResult<()> {
    self.inner.set_name(name).await
  }
  async fn get_label(&self, id: &str) -> SessionResult<Option<String>> {
    self.inner.get_label(id).await
  }
  async fn set_label(&self, id: &str, label: Option<&str>) -> SessionResult<()> {
    self.inner.set_label(id, label).await
  }
  async fn get_stats(&self) -> SessionResult<SessionStats> {
    self.inner.get_stats().await
  }
}

async fn open(storage: Arc<RefuseCancellation>) -> AgentHarness {
  AgentHarness::create(AgentHarnessOptions {
    session: Session::new(storage, None),
    allow_existing_session: true,
    // Empty provider registry and no prompt: this test cannot invoke inference.
    models: Vec::new(),
    ..Default::default()
  })
  .await
  .expect("create harness without executing a prompt")
}

async fn reproduce(clear_all: bool) {
  let storage = Arc::new(RefuseCancellation::new());
  let harness = open(storage.clone()).await;
  let queued = harness
    .steer(AgentMessage::User(UserMessage::new(
      "synthetic queued prompt",
      0,
    )))
    .await
    .expect("durable enqueue accepted");
  if clear_all {
    let returned = harness
      .clear_queue()
      .await
      .expect("upstream incorrectly returns success");
    assert_eq!(returned.steering, vec!["synthetic queued prompt"]);
  } else {
    let returned = harness
      .cancel_queued(&queued.entry_id)
      .await
      .expect("upstream incorrectly returns success");
    assert_eq!(returned.outcome, CancelQueuedOutcome::Cancelled);
  }
  assert_eq!(storage.failures.load(Ordering::SeqCst), 1);
  assert!(
    harness
      .queued_messages()
      .await
      .expect("read current queue")
      .is_empty()
  );
  // The public write-failure channel also omits this cancellation failure.
  assert!(harness.take_write_failures().is_empty());
  let records = storage
    .find_records(&RecordQuery::default())
    .await
    .expect("read storage");
  assert!(
    records
      .iter()
      .any(|record| matches!(record, LaneRecord::QueueEnqueued(_)))
  );
  assert!(
    !records
      .iter()
      .any(|record| matches!(record, LaneRecord::QueueCancelled(_)))
  );
  drop(harness);
  let reopened = open(storage).await;
  assert_eq!(
    reopened
      .queued_messages()
      .await
      .expect("read rebuilt queue")
      .steering,
    vec!["synthetic queued prompt"]
  );
}

#[tokio::test]
async fn cancel_reports_success_despite_storage_failure_and_message_reappears() {
  reproduce(false).await;
}

#[tokio::test]
async fn clear_reports_success_despite_storage_failure_and_message_reappears() {
  reproduce(true).await;
}

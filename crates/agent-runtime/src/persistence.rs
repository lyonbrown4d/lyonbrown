use std::sync::Arc;

use async_trait::async_trait;
use rpi_harness::error::{SessionError, SessionResult};
use rpi_harness::session::DefaultIdGenerator;
use rpi_harness::session::memory::{Clock, InMemorySessionStorage, SystemClock};
use rpi_harness::session::state::SessionState;
use rpi_harness::session::types::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

use crate::{Result, RuntimeError, SessionMutation as Mutation, SessionPersistence};

#[derive(Serialize, Deserialize)]
struct Snapshot {
  version: u32,
  metadata: SessionMetadata,
  state: SessionState,
}

pub(crate) struct DurableSession {
  inner: InMemorySessionStorage,
  gate: Mutex<()>,
  persistence: Arc<dyn SessionPersistence>,
  owner_epoch: u64,
}

impl DurableSession {
  pub(crate) async fn open(
    id: &str,
    owner_epoch: u64,
    persistence: Arc<dyn SessionPersistence>,
  ) -> Result<Self> {
    let snapshot = match persistence.load(id).await? {
      Some(bytes) => {
        crate::image_recovery::check(&bytes)?;
        let snapshot: Snapshot = serde_json::from_slice(&bytes)
          .map_err(|error| RuntimeError::Persistence(error.to_string()))?;
        if snapshot.version != 1 || snapshot.metadata.id != id {
          return Err(RuntimeError::Persistence(
            "invalid session snapshot identity/version".into(),
          ));
        }
        snapshot
      }
      None => Snapshot {
        version: 1,
        metadata: SessionMetadata {
          id: id.into(),
          created_at: SystemClock.now_ms(),
          parent_session_id: None,
        },
        state: SessionState::new(),
      },
    };
    Ok(Self {
      inner: InMemorySessionStorage::from_state(
        snapshot.metadata,
        snapshot.state,
        Arc::new(SystemClock),
        Arc::new(DefaultIdGenerator::new()),
      ),
      gate: Mutex::new(()),
      persistence,
      owner_epoch,
    })
  }

  async fn persist(
    &self,
    before: SessionState,
    kind: &str,
    payload: serde_json::Value,
  ) -> SessionResult<()> {
    let metadata = self.inner.metadata();
    let snapshot = Snapshot {
      version: 1,
      metadata: metadata.clone(),
      state: self.inner.snapshot_state(),
    };
    let encoded =
      serde_json::to_vec(&snapshot).map_err(|error| SessionError::storage(error.to_string()));
    let result = match encoded {
      Ok(bytes) => self
        .persistence
        .commit(
          &metadata.id,
          self.owner_epoch,
          bytes,
          Mutation {
            kind: kind.into(),
            payload,
          },
        )
        .await
        .map_err(|error| SessionError::storage(error.to_string())),
      Err(error) => Err(error),
    };
    if result.is_err() {
      self.inner.replace_state(before);
    }
    result
  }
}

#[async_trait]
impl SessionStorage for DurableSession {
  fn metadata(&self) -> SessionMetadata {
    self.inner.metadata()
  }
  async fn get_lanes(&self) -> SessionResult<Vec<LanePointer>> {
    let _guard = self.gate.lock().await;
    self.inner.get_lanes().await
  }
  async fn get_entry(&self, id: &str) -> SessionResult<Option<Entry>> {
    let _guard = self.gate.lock().await;
    self.inner.get_entry(id).await
  }
  async fn find_entries(&self, query: &EntryQuery) -> SessionResult<Vec<Entry>> {
    let _guard = self.gate.lock().await;
    self.inner.find_entries(query).await
  }
  async fn find_entries_on_branch(
    &self,
    query: &EntryQuery,
    bounds: &BranchBounds,
    start: &str,
  ) -> SessionResult<Vec<Entry>> {
    let _guard = self.gate.lock().await;
    self
      .inner
      .find_entries_on_branch(query, bounds, start)
      .await
  }
  async fn find_records(&self, query: &RecordQuery) -> SessionResult<Vec<LaneRecord>> {
    let _guard = self.gate.lock().await;
    self.inner.find_records(query).await
  }
  async fn find_open_operations(
    &self,
    lane: &str,
    limit: Option<usize>,
  ) -> SessionResult<Vec<OperationStartedRecord>> {
    let _guard = self.gate.lock().await;
    self.inner.find_open_operations(lane, limit).await
  }
  async fn get_log(&self, options: &LogOptions) -> SessionResult<Vec<LogItem>> {
    let _guard = self.gate.lock().await;
    self.inner.get_log(options).await
  }
  async fn get_name(&self) -> SessionResult<Option<String>> {
    let _guard = self.gate.lock().await;
    self.inner.get_name().await
  }
  async fn get_label(&self, id: &str) -> SessionResult<Option<String>> {
    let _guard = self.gate.lock().await;
    self.inner.get_label(id).await
  }
  async fn get_stats(&self) -> SessionResult<SessionStats> {
    let _guard = self.gate.lock().await;
    self.inner.get_stats().await
  }

  async fn create_lane(&self, lane: &str, at: Option<&str>) -> SessionResult<()> {
    let _guard = self.gate.lock().await;
    let before = self.inner.snapshot_state();
    self.inner.create_lane(lane, at).await?;
    self
      .persist(before, "lane_created", json!({"lane": lane, "at": at}))
      .await
  }

  async fn move_lane(&self, lane: &str, to: Option<&str>) -> SessionResult<()> {
    let _guard = self.gate.lock().await;
    let before = self.inner.snapshot_state();
    self.inner.move_lane(lane, to).await?;
    self
      .persist(before, "lane_moved", json!({"lane": lane, "to": to}))
      .await
  }

  async fn append_entry(&self, entry: ProvisionedEntry, lane: &str) -> SessionResult<Entry> {
    let _guard = self.gate.lock().await;
    let before = self.inner.snapshot_state();
    let entry = self.inner.append_entry(entry, lane).await?;
    self
      .persist(
        before,
        "entry_appended",
        json!({"lane": lane, "entry": crate::event_projection::entry(&entry)}),
      )
      .await?;
    Ok(entry)
  }

  async fn append_record(&self, record: LaneRecord) -> SessionResult<LaneRecord> {
    let _guard = self.gate.lock().await;
    let before = self.inner.snapshot_state();
    let record = self.inner.append_record(record).await?;
    self
      .persist(
        before,
        "record_appended",
        json!({"record": crate::event_projection::record(&record)}),
      )
      .await?;
    Ok(record)
  }

  async fn set_name(&self, name: Option<&str>) -> SessionResult<()> {
    let _guard = self.gate.lock().await;
    let before = self.inner.snapshot_state();
    self.inner.set_name(name).await?;
    self
      .persist(before, "name_changed", json!({"name": name}))
      .await
  }

  async fn set_label(&self, id: &str, label: Option<&str>) -> SessionResult<()> {
    let _guard = self.gate.lock().await;
    let before = self.inner.snapshot_state();
    self.inner.set_label(id, label).await?;
    self
      .persist(before, "label_changed", json!({"id": id, "label": label}))
      .await
  }
}

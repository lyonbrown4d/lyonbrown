use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;

use agent_core::{AgentEvent, SessionId};
use tokio::sync::{mpsc, oneshot};

use crate::db;

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct StoreError(pub String);

impl From<std::io::Error> for StoreError {
  fn from(error: std::io::Error) -> Self {
    Self(error.to_string())
  }
}

impl From<rusqlite::Error> for StoreError {
  fn from(error: rusqlite::Error) -> Self {
    Self(error.to_string())
  }
}

impl From<serde_json::Error> for StoreError {
  fn from(error: serde_json::Error) -> Self {
    Self(error.to_string())
  }
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
  pub id: SessionId,
  pub updated_at: String,
  pub event_count: u64,
}

#[derive(Clone)]
pub struct SessionStore {
  path: Arc<PathBuf>,
}

impl SessionStore {
  pub async fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
    let path = path.as_ref().to_owned();
    let worker_path = path.clone();
    tokio::task::spawn_blocking(move || db::initialize(&worker_path))
      .await
      .map_err(|error| StoreError(error.to_string()))??;
    Ok(Self {
      path: Arc::new(path),
    })
  }

  pub fn writer(&self, session_id: SessionId) -> EventWriter {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let path = self.path.clone();
    let join = tokio::task::spawn_blocking(move || {
      let mut connection = db::connect(&path).map_err(|error| error.to_string());
      while let Some(command) = receiver.blocking_recv() {
        match command {
          WriteCommand::Event(event, response) => {
            if let Ok(database) = &mut connection
              && let Err(error) = db::append_event(database, session_id, &event)
            {
              connection = Err(error.to_string());
            }
            let result = connection
              .as_ref()
              .map(|_| ())
              .map_err(|error| StoreError(error.clone()));
            let _ = response.send(result);
          }
          WriteCommand::Flush(response) => {
            let result = connection
              .as_ref()
              .map(|_| ())
              .map_err(|error| StoreError(error.clone()));
            let _ = response.send(result);
          }
        }
      }
    });
    EventWriter { sender, join }
  }

  pub async fn events(&self, session_id: SessionId) -> Result<Vec<AgentEvent>, StoreError> {
    let path = self.path.clone();
    tokio::task::spawn_blocking(move || db::load_events(&path, session_id))
      .await
      .map_err(|error| StoreError(error.to_string()))?
  }

  pub async fn sessions(&self) -> Result<Vec<SessionSummary>, StoreError> {
    let path = self.path.clone();
    tokio::task::spawn_blocking(move || db::list_sessions(&path))
      .await
      .map_err(|error| StoreError(error.to_string()))?
  }

  pub async fn tool_audit_count(&self, session_id: SessionId) -> Result<u64, StoreError> {
    let path = self.path.clone();
    tokio::task::spawn_blocking(move || db::audit_count(&path, session_id))
      .await
      .map_err(|error| StoreError(error.to_string()))?
  }
}

enum WriteCommand {
  Event(AgentEvent, std_mpsc::SyncSender<Result<(), StoreError>>),
  Flush(oneshot::Sender<Result<(), StoreError>>),
}

pub struct EventWriter {
  sender: mpsc::UnboundedSender<WriteCommand>,
  join: tokio::task::JoinHandle<()>,
}

impl EventWriter {
  pub fn record(&self, event: AgentEvent) -> Result<(), StoreError> {
    let (sender, receiver) = std_mpsc::sync_channel(1);
    self
      .sender
      .send(WriteCommand::Event(event, sender))
      .map_err(|_| StoreError("event writer is closed".into()))?;
    receiver
      .recv()
      .map_err(|_| StoreError("event writer stopped before recording".into()))?
  }

  pub async fn flush(&self) -> Result<(), StoreError> {
    let (sender, receiver) = oneshot::channel();
    self
      .sender
      .send(WriteCommand::Flush(sender))
      .map_err(|_| StoreError("event writer is closed".into()))?;
    receiver
      .await
      .map_err(|_| StoreError("event writer stopped before flush".into()))?
  }

  pub async fn close(self) -> Result<(), StoreError> {
    let result = self.flush().await;
    let Self { sender, join } = self;
    drop(sender);
    join.await.map_err(|error| StoreError(error.to_string()))?;
    result
  }
}

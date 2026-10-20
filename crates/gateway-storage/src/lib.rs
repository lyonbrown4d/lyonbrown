//! SQLite metadata persistence. Prompts, completions and provider secrets have no storage fields.
#![forbid(unsafe_code)]
mod access;
mod audit;
mod auth;
mod catalog;
mod error;
mod files;
mod migrations;
mod password;
mod policy;
mod schema;
mod sessions;
pub use access::{AccessTokenMetadata, IssuedAccessToken};
pub use audit::AuditPage;
pub use catalog::{ModelMetadata, ModelSnapshot};
use diesel::{connection::SimpleConnection, prelude::*};
pub use error::{Result, StorageError};
pub use sessions::{AdminIdentity, AdminRole, AdminSession};
use std::{
  fs::File,
  path::Path,
  sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;

struct Inner {
  connection: Mutex<SqliteConnection>,
  permits: Arc<Semaphore>,
  password_permits: Arc<Semaphore>,
  _ownership: File,
}
#[derive(Clone)]
pub struct GatewayStorage {
  inner: Arc<Inner>,
}
impl GatewayStorage {
  /// Startup-only blocking open. An exclusive file lock prevents another owner from reconciling live intents.
  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    let path = path.as_ref();
    let ownership = files::prepare(path)?;
    let path = path
      .to_str()
      .ok_or(StorageError::InvalidInput("non-UTF-8 database path"))?;
    let mut connection = SqliteConnection::establish(path)?;
    connection.batch_execute(
      "PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
    )?;
    connection.immediate_transaction::<_, StorageError, _>(|connection| {
      migrations::run(connection)?;
      audit::reconcile(connection)?;
      Ok(())
    })?;
    Ok(Self {
      inner: Arc::new(Inner {
        connection: Mutex::new(connection),
        permits: Arc::new(Semaphore::new(1)),
        password_permits: Arc::new(Semaphore::new(1)),
        _ownership: ownership,
      }),
    })
  }
  pub(crate) async fn run<T, F>(&self, operation: F) -> Result<T>
  where
    T: Send + 'static,
    F: FnOnce(&mut SqliteConnection) -> Result<T> + Send + 'static,
  {
    let permit = self
      .inner
      .permits
      .clone()
      .acquire_owned()
      .await
      .map_err(|_| StorageError::Worker)?;
    let inner = self.inner.clone();
    tokio::task::spawn_blocking(move || {
      let _permit = permit;
      let mut connection = inner.connection.lock().map_err(|_| StorageError::Worker)?;
      operation(&mut connection)
    })
    .await
    .map_err(|_| StorageError::Worker)?
  }
}

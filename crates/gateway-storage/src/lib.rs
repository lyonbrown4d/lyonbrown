//! SQLite metadata persistence. Prompts, completions and provider secrets have no storage fields.
#![forbid(unsafe_code)]
mod access;
mod audit;
mod auth;
mod budget;
mod catalog;
mod error;
mod files;
mod migrations;
mod policy;
mod schema;
pub use access::{AccessTokenMetadata, IssuedAccessToken};
pub use audit::AuditPage;
pub use catalog::{ModelMetadata, ModelSnapshot};
pub use error::{Result, StorageError};
use sea_orm::{
  ConnectOptions, Database, DatabaseConnection, DatabaseTransaction, TransactionTrait,
};
use sea_orm_migration::MigratorTrait;
use std::{fs::File, path::Path, sync::Arc};

struct Inner {
  connection: DatabaseConnection,
  _ownership: File,
}
#[derive(Clone)]
pub struct GatewayStorage {
  inner: Arc<Inner>,
}
impl GatewayStorage {
  /// Open the asynchronous SQLite pool and apply the current versioned schema.
  pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
    let path = path.as_ref().to_path_buf();
    let prepared = path.clone();
    let ownership = tokio::task::spawn_blocking(move || files::prepare(&prepared))
      .await
      .map_err(|_| StorageError::Worker)??;
    // The public driver-options hook keeps filesystem paths out of URL parsing
    // and applies durability settings to every pooled connection.
    let mut options = ConnectOptions::new("sqlite://gateway.sqlite?mode=rwc");
    options
      .max_connections(1)
      .min_connections(1)
      .sqlx_logging(false)
      .record_stmt_in_spans(false)
      .map_sqlx_sqlite_opts(move |options| {
        options
          .filename(&path)
          .create_if_missing(true)
          .busy_timeout(std::time::Duration::from_secs(5))
          .journal_mode(sea_orm::sqlx::sqlite::SqliteJournalMode::Wal)
          .synchronous(sea_orm::sqlx::sqlite::SqliteSynchronous::Full)
      });
    let connection = Database::connect(options).await?;
    migrations::Migrator::up(&connection, None).await?;
    audit::reconcile(&connection).await?;
    Ok(Self {
      inner: Arc::new(Inner {
        connection,
        _ownership: ownership,
      }),
    })
  }
  pub(crate) async fn run<T, F>(&self, operation: F) -> Result<T>
  where
    T: Send,
    F: for<'a> FnOnce(
        &'a DatabaseTransaction,
      )
        -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>
      + Send,
  {
    self.transact(false, operation).await
  }
  pub(crate) async fn write<T, F>(&self, operation: F) -> Result<T>
  where
    T: Send,
    F: for<'a> FnOnce(
        &'a DatabaseTransaction,
      )
        -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>
      + Send,
  {
    self.transact(true, operation).await
  }
  async fn transact<T, F>(&self, write: bool, operation: F) -> Result<T>
  where
    T: Send,
    F: for<'a> FnOnce(
        &'a DatabaseTransaction,
      )
        -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>
      + Send,
  {
    // Immediate write transactions acquire the reservation lock before reading budget/CAS state.
    let transaction = self
      .inner
      .connection
      .begin_with_options(sea_orm::TransactionOptions {
        sqlite_transaction_mode: Some(if write {
          sea_orm::SqliteTransactionMode::Immediate
        } else {
          sea_orm::SqliteTransactionMode::Deferred
        }),
        ..Default::default()
      })
      .await?;
    match operation(&transaction).await {
      Ok(value) => {
        transaction.commit().await?;
        Ok(value)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
}

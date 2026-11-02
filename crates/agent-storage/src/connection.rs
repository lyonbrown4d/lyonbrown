use crate::{Store, migration};
use anyhow::Result;
use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sea_orm::{ConnectOptions, Database};
use sea_orm_migration::MigratorTrait;
use std::path::Path;
impl Store {
  /// Filesystem paths are passed as paths, never interpolated into a database URL.
  /// The SQLx driver reports unsupported non-UTF-8 paths without lossy rewriting.
  pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
    let path = path.as_ref().to_path_buf();
    let memory = path == Path::new(":memory:");
    let mut options = connection_options(if memory {
      "sqlite::memory:"
    } else {
      "sqlite://agent.sqlite?mode=rwc"
    });
    if !memory {
      options.map_sqlx_sqlite_opts(move |options| {
        sqlite_options(options.filename(&path).create_if_missing(true))
      });
    }
    Self::initialize(options).await
  }
  pub async fn connect(database_url: &str) -> Result<Self> {
    Self::initialize(connection_options(database_url)).await
  }
  async fn initialize(options: ConnectOptions) -> Result<Self> {
    let connection = Database::connect(options).await?;
    migration::Migrator::up(&connection, None).await?;
    Ok(Self { connection })
  }
}
fn connection_options(url: &str) -> ConnectOptions {
  let mut options = ConnectOptions::new(url);
  options.sqlx_logging(false).record_stmt_in_spans(false);
  if url.starts_with("sqlite:") {
    options
      .max_connections(1)
      .min_connections(1)
      .map_sqlx_sqlite_opts(sqlite_options);
  }
  options
}
fn sqlite_options(options: SqliteConnectOptions) -> SqliteConnectOptions {
  options
    .busy_timeout(std::time::Duration::from_secs(5))
    .journal_mode(SqliteJournalMode::Wal)
    .synchronous(SqliteSynchronous::Full)
}

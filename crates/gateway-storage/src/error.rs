use thiserror::Error;
#[derive(Debug, Error)]
pub enum StorageError {
  #[error("gateway file operation failed")]
  Io(#[from] std::io::Error),
  #[error("gateway database connection failed")]
  Connection(#[from] diesel::ConnectionError),
  #[error("gateway database operation failed")]
  Database(#[from] diesel::result::Error),
  #[error("gateway database migration failed")]
  Migration(#[source] Box<dyn std::error::Error + Send + Sync>),
  #[error("invalid storage input: {0}")]
  InvalidInput(&'static str),
  #[error("gateway storage state conflict")]
  Conflict,
  #[error("authentication failed")]
  Unauthorized,
  #[error("gateway storage worker unavailable")]
  Worker,
  #[error("gateway storage data is invalid")]
  InvalidData,
}
pub type Result<T> = std::result::Result<T, StorageError>;
pub(crate) fn valid_key(value: &str) -> bool {
  !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

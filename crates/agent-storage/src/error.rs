/// Detect ORM failures through anyhow context and error source wrappers.
/// The original error may include SQL, connection URLs or driver diagnostics.
pub fn is_database_error(error: &anyhow::Error) -> bool {
  error.chain().any(|cause| cause.is::<sea_orm::DbErr>())
}
/// Fixed text suitable for client errors and operational logs.
pub fn safe_storage_error(error: &anyhow::Error) -> &'static str {
  if is_database_error(error) {
    "database operation unavailable"
  } else {
    "storage operation failed"
  }
}
/// Preserve business validation messages while redacting database diagnostics and
/// JSON errors, which can include excerpts of stored values. This deliberately
/// does not format Debug or the source chain into the public message.
pub fn public_error_message(error: &anyhow::Error) -> String {
  if is_database_error(error) {
    "database operation unavailable".into()
  } else if error.chain().any(|cause| cause.is::<serde_json::Error>()) {
    "stored data is invalid".into()
  } else {
    error.to_string()
  }
}

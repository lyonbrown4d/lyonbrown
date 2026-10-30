use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

/// Returns the credential-free, canonical scope used by snapshot files.
///
/// Clients can compare authenticated connection scopes before opening a store,
/// without treating equivalent endpoint spellings as different identities.
pub fn identity(client_kind: &str, endpoint: &str, node_id: &str) -> Result<String> {
  ensure!(
    matches!(client_kind, "tui" | "desktop"),
    "unsupported client kind"
  );
  ensure!(
    !node_id.trim().is_empty() && node_id.len() <= 256,
    "invalid daemon node id"
  );
  ensure!(endpoint.len() <= 2048, "daemon endpoint is too long");
  let url = url::Url::parse(endpoint).context("invalid daemon endpoint")?;
  ensure!(
    matches!(url.scheme(), "http" | "https")
      && url.host_str().is_some()
      && url.username().is_empty()
      && url.password().is_none()
      && url.query().is_none()
      && url.fragment().is_none(),
    "invalid daemon endpoint"
  );
  let endpoint = url.as_str();
  let mut hasher = Sha256::new();
  for component in [
    client_kind.as_bytes(),
    endpoint.as_bytes(),
    node_id.as_bytes(),
  ] {
    hasher.update((component.len() as u64).to_be_bytes());
    hasher.update(component);
  }
  Ok(hex::encode(hasher.finalize()))
}

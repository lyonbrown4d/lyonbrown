//! Local identity, process exclusivity and authenticated transport boundaries.
mod identity;
pub use identity::{Identity, NodeLease, token};

/// Select the application TLS provider before constructing clients or servers.
/// Feature unification may enable both ring and aws-lc via independent SDKs;
/// rustls cannot infer a default in that case. Respect an embedding host's
/// already-installed provider, including a concurrent successful installation.
pub fn initialize_tls() {
  if rustls::crypto::CryptoProvider::get_default().is_none() {
    let _ = rustls::crypto::ring::default_provider().install_default();
  }
}
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tonic::{Request, Status, service::Interceptor};
#[derive(Clone)]
pub struct Authenticate {
  digest: [u8; 32],
}
impl Authenticate {
  pub fn new(token: &str) -> Self {
    Self {
      digest: Sha256::digest(format!("Bearer {}", token.trim()).as_bytes()).into(),
    }
  }
}
impl Interceptor for Authenticate {
  fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
    let auth = request
      .metadata()
      .get("authorization")
      .map(|v| v.as_bytes())
      .unwrap_or_default();
    let supplied: [u8; 32] = Sha256::digest(auth).into();
    if bool::from(self.digest.ct_eq(&supplied)) {
      Ok(request)
    } else {
      Err(Status::unauthenticated(
        "valid daemon credentials are required",
      ))
    }
  }
}

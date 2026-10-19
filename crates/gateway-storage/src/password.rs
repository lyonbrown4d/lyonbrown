use crate::{GatewayStorage, Result, StorageError};
use argon2::{
  Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
  password_hash::SaltString,
};
use std::sync::LazyLock;

// A non-credential fixture, generated once by this implementation with the same parameters.
// Always initialize it before verification, regardless of whether the username exists.
static DUMMY_PHC: LazyLock<std::result::Result<String, ()>> = LazyLock::new(|| {
  hash_with_salt(b"gateway-dummy-not-an-account-v2", b"gateway-dummy-v2").map_err(|_| ())
});

impl GatewayStorage {
  pub(crate) async fn hash_password(&self, password: String) -> Result<String> {
    self
      .password_work(move || {
        let mut salt = [0_u8; 16];
        getrandom::fill(&mut salt).map_err(|_| StorageError::Worker)?;
        hash_with_salt(password.as_bytes(), &salt)
      })
      .await
  }
  pub(crate) async fn verify_password(
    &self,
    password: String,
    phc: Option<String>,
  ) -> Result<bool> {
    self
      .password_work(move || {
        let dummy = LazyLock::force(&DUMMY_PHC)
          .as_ref()
          .map_err(|_| StorageError::Worker)?;
        let phc = phc.as_deref().unwrap_or(dummy);
        // Only our bounded Argon2id parameter set is supported; never trust database PHC costs.
        if phc.len() > 256 || !phc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$") {
          return Err(StorageError::InvalidData);
        }
        let parsed = PasswordHash::new(phc).map_err(|_| StorageError::InvalidData)?;
        Ok(
          argon()?
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        )
      })
      .await
  }
  async fn password_work<T, F>(&self, work: F) -> Result<T>
  where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
  {
    let permit = self
      .inner
      .password_permits
      .clone()
      .acquire_owned()
      .await
      .map_err(|_| StorageError::Worker)?;
    // Only the permit moves into this worker: no SQLite mutex or connection is held during hashing.
    tokio::task::spawn_blocking(move || {
      let _permit = permit;
      work()
    })
    .await
    .map_err(|_| StorageError::Worker)?
  }
}
fn argon() -> Result<Argon2<'static>> {
  // OWASP baseline: Argon2id, 19 MiB memory, two iterations, one lane, 32-byte output.
  let params = Params::new(19_456, 2, 1, Some(32)).map_err(|_| StorageError::Worker)?;
  Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}
fn hash_with_salt(password: &[u8], salt: &[u8]) -> Result<String> {
  let salt = SaltString::encode_b64(salt).map_err(|_| StorageError::Worker)?;
  argon()?
    .hash_password(password, &salt)
    .map(|hash| hash.to_string())
    .map_err(|_| StorageError::Worker)
}

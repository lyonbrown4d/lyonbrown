use crate::{Result, StorageError};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub(crate) fn digest(purpose: &str, token: &str) -> String {
  let mut digest = Sha256::new();
  digest.update(b"lyonbrown-gateway-v1\0");
  digest.update(purpose.as_bytes());
  digest.update(b"\0");
  digest.update(token.as_bytes());
  hex::encode(digest.finalize())
}
pub(crate) fn equal(left: &str, right: &str) -> bool {
  bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}
pub(crate) fn valid_token(token: &str) -> bool {
  (32..=512).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}
pub(crate) fn random_token() -> Result<String> {
  let mut bytes = [0_u8; 32];
  getrandom::fill(&mut bytes).map_err(|_| StorageError::Worker)?;
  Ok(hex::encode(bytes))
}
pub(crate) fn signed_time(now: u64) -> Result<i64> {
  i64::try_from(now).map_err(|_| StorageError::InvalidInput("timestamp"))
}
pub(crate) fn now_seconds() -> Result<u64> {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|elapsed| elapsed.as_secs())
    .map_err(|_| StorageError::Worker)
}

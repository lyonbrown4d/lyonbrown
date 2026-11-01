use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::AeadInPlace};
use rand_core::{OsRng, RngCore};
use secrecy::{ExposeSecret, SecretBox, SecretString};
use zeroize::Zeroizing;

use crate::{MAX_SECRET_BYTES, Result, SecretError};

const MAGIC: &[u8; 8] = b"LYSCRT01";
const NONCE_BYTES: usize = 24;
pub(crate) const MAX_RECORD_BYTES: usize = MAGIC.len() + NONCE_BYTES + MAX_SECRET_BYTES + 16;

pub(crate) fn random<const N: usize>() -> Result<Zeroizing<[u8; N]>> {
  let mut bytes = Zeroizing::new([0; N]);
  OsRng
    .try_fill_bytes(bytes.as_mut())
    .map_err(|_| SecretError::Randomness)?;
  Ok(bytes)
}

pub(crate) fn seal(key: &SecretBox<[u8; 32]>, id: &str, secret: &SecretString) -> Result<Vec<u8>> {
  let nonce = random::<NONCE_BYTES>()?;
  let cipher = XChaCha20Poly1305::new(key.expose_secret().into());
  let mut ciphertext = Zeroizing::new(secret.expose_secret().as_bytes().to_vec());
  cipher
    .encrypt_in_place(
      XNonce::from_slice(nonce.as_ref()),
      &associated_data(id),
      &mut *ciphertext,
    )
    .map_err(|_| SecretError::Authentication)?;
  let mut record = Vec::with_capacity(MAGIC.len() + NONCE_BYTES + ciphertext.len());
  record.extend_from_slice(MAGIC);
  record.extend_from_slice(nonce.as_ref());
  record.extend_from_slice(&ciphertext);
  Ok(record)
}

pub(crate) fn open(key: &SecretBox<[u8; 32]>, id: &str, record: &[u8]) -> Result<SecretString> {
  if record.len() <= MAGIC.len() + NONCE_BYTES + 16
    || record.len() > MAX_RECORD_BYTES
    || &record[..MAGIC.len()] != MAGIC
  {
    return Err(SecretError::InvalidRecord);
  }
  let cipher = XChaCha20Poly1305::new(key.expose_secret().into());
  let nonce = XNonce::from_slice(&record[MAGIC.len()..MAGIC.len() + NONCE_BYTES]);
  let mut plaintext = Zeroizing::new(record[MAGIC.len() + NONCE_BYTES..].to_vec());
  cipher
    .decrypt_in_place(nonce, &associated_data(id), &mut *plaintext)
    .map_err(|_| SecretError::Authentication)?;
  let value = std::str::from_utf8(&plaintext).map_err(|_| SecretError::InvalidSecret)?;
  validate_secret(value)?;
  Ok(SecretString::from(value.to_owned()))
}

pub(crate) fn validate_secret(value: &str) -> Result<()> {
  if value.trim().is_empty() || value.len() > MAX_SECRET_BYTES || value.contains('\0') {
    return Err(SecretError::InvalidSecret);
  }
  Ok(())
}

fn associated_data(id: &str) -> Vec<u8> {
  let mut data = b"lyonbrown/provider-secret/v1\0".to_vec();
  data.extend_from_slice(id.as_bytes());
  data
}

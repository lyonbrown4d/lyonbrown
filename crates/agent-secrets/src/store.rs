use std::{
  fs,
  path::{Path, PathBuf},
};

use secrecy::{ExposeSecret, SecretBox, SecretString};
use zeroize::Zeroizing;

use crate::{Result, SecretError, SecretMetadata, crypto, files, validate_id};

/// The host service must record execution intent before calling mutating methods.
/// This object intentionally implements neither Debug nor serialization.
pub struct SecretStore {
  root: PathBuf,
  key: SecretBox<[u8; 32]>,
}

impl SecretStore {
  /// Opens a dedicated private directory and provisions one local master key.
  /// Existing encrypted records without a key fail closed.
  pub fn open(root: impl AsRef<Path>) -> Result<Self> {
    files::root(root.as_ref())?;
    let root = root.as_ref().canonicalize()?;
    let _lock = files::lock(&root)?;
    let key_path = root.join("master.key");
    let bytes = match files::read(&key_path, 32)? {
      Some(bytes) => Zeroizing::new(bytes),
      None => {
        if has_records(&root)? {
          return Err(SecretError::InvalidKey);
        }
        let key = crypto::random::<32>()?;
        files::atomic_write(&root, &key_path, key.as_ref(), true)?;
        Zeroizing::new(key.to_vec())
      }
    };
    if bytes.len() != 32 {
      return Err(SecretError::InvalidKey);
    }
    let mut key = Box::new([0; 32]);
    key.copy_from_slice(&bytes);
    Ok(Self {
      root,
      key: SecretBox::new(key),
    })
  }

  /// Creates or replaces a credential using a new random nonce on every write.
  pub fn put(&self, id: &str, secret: &SecretString) -> Result<SecretMetadata> {
    validate_id(id)?;
    crypto::validate_secret(secret.expose_secret())?;
    let _lock = files::lock(&self.root)?;
    let path = self.path(id);
    if let Some(record) = files::read(&path, crypto::MAX_RECORD_BYTES)? {
      // Refuse to overwrite a corrupted record before the host reconciles it.
      drop(crypto::open(&self.key, id, &record)?);
    }
    let record = crypto::seal(&self.key, id, secret)?;
    files::atomic_write(&self.root, &path, &record, false)?;
    Ok(SecretMetadata::new(id))
  }

  /// Plaintext is returned only through an explicit, zeroizing secret wrapper.
  pub fn get(&self, id: &str) -> Result<Option<SecretString>> {
    validate_id(id)?;
    let _lock = files::lock(&self.root)?;
    self.read(id)
  }

  /// Lists authenticated credential identities without values or value lengths.
  pub fn list(&self) -> Result<Vec<SecretMetadata>> {
    let _lock = files::lock(&self.root)?;
    let mut metadata = Vec::new();
    for entry in fs::read_dir(&self.root)? {
      let entry = entry?;
      let name = entry.file_name();
      let Some(name) = name.to_str() else { continue };
      let Some(id) = name.strip_suffix(".secret") else {
        continue;
      };
      validate_id(id)?;
      drop(self.read(id)?.ok_or(SecretError::InvalidRecord)?);
      metadata.push(SecretMetadata::new(id));
    }
    metadata.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(metadata)
  }

  /// Removes only the named authenticated record. The caller retains any audit
  /// receipt; this does not guarantee forensic erasure on the underlying media.
  pub fn remove(&self, id: &str) -> Result<bool> {
    validate_id(id)?;
    let _lock = files::lock(&self.root)?;
    if self.read(id)?.is_none() {
      return Ok(false);
    }
    fs::remove_file(self.path(id))?;
    files::sync(&self.root)?;
    Ok(true)
  }

  fn read(&self, id: &str) -> Result<Option<SecretString>> {
    files::read(&self.path(id), crypto::MAX_RECORD_BYTES)?
      .map(|record| crypto::open(&self.key, id, &record))
      .transpose()
  }

  fn path(&self, id: &str) -> PathBuf {
    self.root.join(format!("{id}.secret"))
  }
}

fn has_records(root: &Path) -> Result<bool> {
  for entry in fs::read_dir(root)? {
    if entry?
      .path()
      .extension()
      .is_some_and(|extension| extension == "secret")
    {
      return Ok(true);
    }
  }
  Ok(false)
}

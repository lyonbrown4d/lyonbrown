use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
  fs::{self, File, OpenOptions},
  io::{Read, Write},
  path::Path,
};
#[derive(Serialize, Deserialize)]
pub struct Identity {
  pub node_id: String,
}
pub struct NodeLease {
  _file: File,
}
impl NodeLease {
  pub fn acquire(data: &Path) -> Result<Self> {
    fs::create_dir_all(data)?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(data, fs::Permissions::from_mode(0o700))?;
    }
    let file = OpenOptions::new()
      .create(true)
      .read(true)
      .write(true)
      .truncate(false)
      .open(data.join("agentd.lock"))?;
    file
      .try_lock_exclusive()
      .context("another agentd owns this data directory")?;
    Ok(Self { _file: file })
  }
}
impl Identity {
  pub fn load(data: &Path) -> Result<Self> {
    let path = data.join("identity.json");
    if path.exists() {
      return Ok(serde_json::from_slice(&fs::read(path)?)?);
    }
    let identity = Self {
      node_id: agent_core::new_id(),
    };
    private_create(&path, &serde_json::to_vec(&identity)?)?;
    Ok(identity)
  }
}
pub fn token(data: &Path) -> Result<String> {
  let path = data.join("auth.token");
  match private_create(
    &path,
    format!(
      "{}{}",
      uuid::Uuid::new_v4().simple(),
      uuid::Uuid::new_v4().simple()
    )
    .as_bytes(),
  ) {
    Ok(()) => {}
    Err(e) if path.exists() => {
      drop(e);
    }
    Err(e) => return Err(e),
  }
  let mut value = String::new();
  File::open(path)?.read_to_string(&mut value)?;
  anyhow::ensure!(value.trim().len() >= 32, "invalid daemon token file");
  Ok(value.trim().into())
}
fn private_create(path: &Path, bytes: &[u8]) -> Result<()> {
  let mut options = OpenOptions::new();
  options.create_new(true).write(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
  }
  let mut file = options.open(path)?;
  file.write_all(bytes)?;
  file.sync_all()?;
  Ok(())
}

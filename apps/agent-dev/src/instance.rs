use crate::Plan;
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
  collections::BTreeMap,
  fs::{self, File, OpenOptions},
  io::{Read, Write},
  path::Path,
};

pub(crate) struct BootstrapLease {
  _file: File,
}
impl BootstrapLease {
  pub fn try_acquire(plan: &Plan) -> Result<Option<Self>> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(0o600);
    }
    let file = options.open(plan.data_dir.join("dev-bootstrap.lock"))?;
    match file.try_lock_exclusive() {
      Ok(()) => Ok(Some(Self { _file: file })),
      Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => Ok(None),
      Err(error) => Err(error).context("lock development bootstrap"),
    }
  }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Instance {
  pub node_id: String,
  pub endpoint: String,
  pub binary_digest: String,
  pub configuration_digest: String,
  pub incarnation: String,
}
impl Instance {
  pub fn expected(
    plan: &Plan,
    binary: &Path,
    ambient: &BTreeMap<String, String>,
    incarnation: String,
  ) -> Result<Self> {
    Ok(Self {
      node_id: agent_security::Identity::load(&plan.data_dir)?.node_id,
      endpoint: plan.endpoint(),
      binary_digest: binary_digest(binary)?,
      configuration_digest: configuration_digest(plan, ambient)?,
      incarnation,
    })
  }
  pub fn verify_record(&self, plan: &Plan) -> Result<Self> {
    let bytes = fs::read(plan.data_dir.join("dev-instance.json"))
      .context("listener is not a verified development instance; stop it from its owning terminal or choose another --listen and --data-dir")?;
    let record: Self =
      serde_json::from_slice(&bytes).context("invalid development instance record")?;
    ensure!(
      record.node_id == self.node_id
        && record.endpoint == self.endpoint
        && record.binary_digest == self.binary_digest
        && record.configuration_digest == self.configuration_digest,
      "live development daemon uses a different binary, configuration, identity or endpoint; stop it from its owning terminal before restarting"
    );
    ensure!(
      record.incarnation.len() == 32
        && record
          .incarnation
          .bytes()
          .all(|byte| byte.is_ascii_hexdigit()),
      "invalid development instance incarnation; stop it from its owning terminal before restarting"
    );
    Ok(record)
  }
  pub fn publish(&self, plan: &Plan) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(&plan.data_dir)?;
    file.write_all(&serde_json::to_vec(self)?)?;
    file.as_file().sync_all()?;
    file.persist(plan.data_dir.join("dev-instance.json"))?;
    Ok(())
  }
}

pub(crate) fn read_token(plan: &Plan) -> Result<String> {
  let path = plan.data_dir.join("auth.token");
  let metadata = fs::symlink_metadata(&path).context("read development auth.token")?;
  ensure!(
    metadata.is_file(),
    "development auth.token must be a regular private file"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      metadata.permissions().mode() & 0o077 == 0,
      "development auth.token must be private (chmod 600)"
    );
  }
  let token = fs::read_to_string(path).context("read development credentials")?;
  ensure!(token.trim().len() >= 32, "invalid development auth.token");
  Ok(token.trim().to_owned())
}

fn configuration_digest(plan: &Plan, ambient: &BTreeMap<String, String>) -> Result<String> {
  let settings = agent_config::load_with_environment(
    &plan.data_dir,
    plan.config.as_deref(),
    &plan.environment(ambient),
  )?;
  let mut hash = Sha256::new();
  let path = plan
    .config
    .clone()
    .unwrap_or_else(|| plan.data_dir.join("config.toml"));
  hash.update(path.as_os_str().as_encoded_bytes());
  for file in [&path, &path.parent().unwrap_or(&plan.data_dir).join(".env")] {
    match fs::read(file) {
      Ok(bytes) => {
        hash.update(b"present\0");
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
      }
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => hash.update(b"absent\0"),
      Err(error) => return Err(error).context("read development configuration identity"),
    }
  }
  hash.update(format!("{settings:?}"));
  // Credentials are only fed into the hash; no resolved secret is written to the record.
  for provider in ["openai", "anthropic"] {
    hash.update(settings.credential(provider).unwrap_or_default().as_bytes());
  }
  for reference in settings.commands.environment.values() {
    hash.update(
      settings
        .command_environment_value(reference)
        .unwrap_or_default()
        .as_bytes(),
    );
  }
  for server in &settings.mcp {
    for reference in server.environment_references() {
      hash.update(
        settings
          .mcp_environment_value(reference)
          .unwrap_or_default()
          .as_bytes(),
      );
    }
  }
  Ok(hex::encode(hash.finalize()))
}

fn binary_digest(path: &Path) -> Result<String> {
  let mut file = File::open(path).context("read built agentd")?;
  let mut hash = Sha256::new();
  let mut bytes = [0u8; 65_536];
  loop {
    let count = file.read(&mut bytes)?;
    if count == 0 {
      break;
    }
    hash.update(&bytes[..count]);
  }
  Ok(hex::encode(hash.finalize()))
}

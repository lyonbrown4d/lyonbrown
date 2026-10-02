use crate::LaunchError;
use agent_client::AuthenticatedTarget;
use agent_local::{DiscoveryDescriptor, ScopeFingerprint};
use sha2::{Digest as _, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LaunchRequest {
  canonical_workspace: PathBuf,
  runtime_config_fingerprint: String,
  scope_fingerprint: ScopeFingerprint,
  auto_start: bool,
  timeout: Duration,
}

impl LaunchRequest {
  pub fn new(
    canonical_workspace: PathBuf,
    runtime_config_fingerprint: impl Into<String>,
    auto_start: bool,
    timeout: Duration,
  ) -> Result<Self, LaunchError> {
    validate_workspace(&canonical_workspace)?;
    let runtime_config_fingerprint = runtime_config_fingerprint.into();
    if runtime_config_fingerprint.trim().is_empty() {
      return Err(LaunchError::InvalidRequest(
        "runtime config fingerprint must not be empty",
      ));
    }
    if timeout.is_zero() {
      return Err(LaunchError::InvalidRequest("timeout must be non-zero"));
    }
    let scope_fingerprint = make_scope(&canonical_workspace, &runtime_config_fingerprint)?;
    Ok(Self {
      canonical_workspace,
      runtime_config_fingerprint,
      scope_fingerprint,
      auto_start,
      timeout,
    })
  }

  pub fn canonical_workspace(&self) -> &Path {
    &self.canonical_workspace
  }

  pub fn runtime_config_fingerprint(&self) -> &str {
    &self.runtime_config_fingerprint
  }

  pub fn scope_fingerprint(&self) -> &ScopeFingerprint {
    &self.scope_fingerprint
  }

  pub fn auto_start(&self) -> bool {
    self.auto_start
  }

  pub fn timeout(&self) -> Duration {
    self.timeout
  }
}

#[derive(Debug, Clone)]
pub struct SpawnRequest {
  canonical_workspace: PathBuf,
  runtime_config_fingerprint: String,
  scope_fingerprint: ScopeFingerprint,
}

impl SpawnRequest {
  pub fn from_launch_request(request: &LaunchRequest) -> Self {
    Self {
      canonical_workspace: request.canonical_workspace.clone(),
      runtime_config_fingerprint: request.runtime_config_fingerprint.clone(),
      scope_fingerprint: request.scope_fingerprint.clone(),
    }
  }

  pub fn canonical_workspace(&self) -> &Path {
    &self.canonical_workspace
  }

  pub fn runtime_config_fingerprint(&self) -> &str {
    &self.runtime_config_fingerprint
  }

  pub fn scope_fingerprint(&self) -> &ScopeFingerprint {
    &self.scope_fingerprint
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
  server_instance_id: Uuid,
  scope_fingerprint: ScopeFingerprint,
}

impl Handshake {
  pub fn new(server_instance_id: Uuid, scope_fingerprint: ScopeFingerprint) -> Self {
    Self {
      server_instance_id,
      scope_fingerprint,
    }
  }

  pub fn server_instance_id(&self) -> Uuid {
    self.server_instance_id
  }

  pub fn scope_fingerprint(&self) -> &ScopeFingerprint {
    &self.scope_fingerprint
  }
}

#[derive(Debug, Clone)]
pub struct DaemonTarget {
  descriptor: DiscoveryDescriptor,
  endpoint: String,
}

impl DaemonTarget {
  pub(crate) fn new(descriptor: DiscoveryDescriptor) -> Self {
    let endpoint = format!("http://{}", descriptor.address());
    Self {
      descriptor,
      endpoint,
    }
  }

  pub fn descriptor(&self) -> &DiscoveryDescriptor {
    &self.descriptor
  }

  pub fn endpoint(&self) -> &str {
    &self.endpoint
  }

  pub fn authenticated_target(&self) -> AuthenticatedTarget {
    AuthenticatedTarget::from_descriptor(&self.descriptor)
  }
}

fn validate_workspace(path: &Path) -> Result<(), LaunchError> {
  if !path.is_absolute() {
    return Err(LaunchError::InvalidRequest(
      "canonical workspace must be absolute",
    ));
  }
  if path
    .components()
    .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
  {
    return Err(LaunchError::InvalidRequest(
      "canonical workspace must not contain dot components",
    ));
  }
  Ok(())
}

fn make_scope(path: &Path, runtime_fingerprint: &str) -> Result<ScopeFingerprint, LaunchError> {
  let workspace_identity = agent_workspace::workspace_identity(path)?;
  let mut digest = Sha256::new();
  digest.update(b"lyonbrown-agentd-scope-v1\0");
  digest.update(workspace_identity.len().to_le_bytes());
  digest.update(&workspace_identity);
  digest.update(runtime_fingerprint.len().to_le_bytes());
  digest.update(runtime_fingerprint.as_bytes());
  ScopeFingerprint::parse(&format!("{:x}", digest.finalize())).map_err(LaunchError::from)
}

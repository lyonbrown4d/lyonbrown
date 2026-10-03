use crate::WorkspaceError;
#[cfg(any(unix, windows))]
use file_id::FileId;
use std::path::{Path, PathBuf};

/// Returns a machine-local identity for a workspace directory.
///
/// Equivalent path spellings and filesystem aliases resolve to the same identity.
pub fn workspace_identity(path: impl AsRef<Path>) -> Result<Vec<u8>, WorkspaceError> {
  platform_identity(path.as_ref())
}

fn io_error(path: &Path, source: std::io::Error) -> WorkspaceError {
  WorkspaceError::Io {
    path: path.to_path_buf(),
    source,
  }
}

fn ensure_directory(path: &Path) -> Result<(), WorkspaceError> {
  let metadata = path.metadata().map_err(|source| io_error(path, source))?;
  if metadata.is_dir() {
    Ok(())
  } else {
    Err(WorkspaceError::NotDirectory {
      path: PathBuf::from(path),
    })
  }
}

#[cfg(any(unix, windows))]
fn platform_identity(path: &Path) -> Result<Vec<u8>, WorkspaceError> {
  ensure_directory(path)?;
  let file_id = file_id::get_file_id(path).map_err(|source| io_error(path, source))?;
  Ok(encode_file_id(file_id))
}

#[cfg(any(unix, windows))]
pub(crate) fn encode_file_id(file_id: FileId) -> Vec<u8> {
  let mut identity = b"workspace-file-id-v1\0".to_vec();
  match file_id {
    FileId::Inode {
      device_id,
      inode_number,
    } => {
      identity.push(1);
      identity.extend_from_slice(&device_id.to_be_bytes());
      identity.extend_from_slice(&inode_number.to_be_bytes());
    }
    FileId::LowRes {
      volume_serial_number,
      file_index,
    } => {
      identity.push(2);
      identity.extend_from_slice(&volume_serial_number.to_be_bytes());
      identity.extend_from_slice(&file_index.to_be_bytes());
    }
    FileId::HighRes {
      volume_serial_number,
      file_id,
    } => {
      identity.push(3);
      identity.extend_from_slice(&volume_serial_number.to_be_bytes());
      identity.extend_from_slice(&file_id.to_be_bytes());
    }
  }
  identity
}

#[cfg(not(any(unix, windows)))]
fn platform_identity(path: &Path) -> Result<Vec<u8>, WorkspaceError> {
  ensure_directory(path)?;
  let canonical = path
    .canonicalize()
    .map_err(|source| io_error(path, source))?;
  let mut identity = b"workspace-path-v1\0".to_vec();
  identity.extend_from_slice(canonical.to_string_lossy().as_bytes());
  Ok(identity)
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
  use super::{encode_file_id, workspace_identity};
  use file_id::FileId;
  use serde::Deserialize;
  use sha2::{Digest as _, Sha256};

  #[derive(Deserialize)]
  struct Vector {
    name: String,
    identity: Identity,
    identity_hex: String,
    config_fingerprint: String,
    scope_id: String,
  }

  #[derive(Deserialize)]
  #[serde(tag = "kind")]
  enum Identity {
    #[serde(rename = "inode")]
    Inode {
      device_id: String,
      inode_number: String,
    },
    #[serde(rename = "windows-low-res")]
    LowRes {
      volume_serial_number: String,
      file_index: String,
    },
    #[serde(rename = "windows-high-res")]
    HighRes {
      volume_serial_number: String,
      file_id: String,
    },
  }

  fn parse_hex_u64(value: &str) -> u64 {
    u64::from_str_radix(value.trim_start_matches("0x"), 16).expect("u64 fixture field")
  }

  fn parse_hex_u128(value: &str) -> u128 {
    u128::from_str_radix(value.trim_start_matches("0x"), 16).expect("u128 fixture field")
  }

  fn file_id(identity: &Identity) -> FileId {
    match identity {
      Identity::Inode {
        device_id,
        inode_number,
      } => FileId::Inode {
        device_id: parse_hex_u64(device_id),
        inode_number: parse_hex_u64(inode_number),
      },
      Identity::LowRes {
        volume_serial_number,
        file_index,
      } => FileId::LowRes {
        volume_serial_number: u32::try_from(parse_hex_u64(volume_serial_number))
          .expect("u32 fixture field"),
        file_index: parse_hex_u64(file_index),
      },
      Identity::HighRes {
        volume_serial_number,
        file_id,
      } => FileId::HighRes {
        volume_serial_number: parse_hex_u64(volume_serial_number),
        file_id: parse_hex_u128(file_id),
      },
    }
  }

  fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
  }

  fn scope(identity: &[u8], config_fingerprint: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"lyonbrown-agentd-scope-v1\0");
    digest.update(identity.len().to_le_bytes());
    digest.update(identity);
    digest.update(config_fingerprint.len().to_le_bytes());
    digest.update(config_fingerprint.as_bytes());
    format!("{:x}", digest.finalize())
  }

  #[test]
  fn committed_typescript_vectors_use_the_production_identity_encoding() {
    let fixture = include_str!(
      "../../../packages/local-control-plane/tests/fixtures/rust-workspace-identity-vectors.json"
    );
    let vectors: Vec<Vector> = serde_json::from_str(fixture).expect("valid identity fixture");
    for vector in &vectors {
      let identity = encode_file_id(file_id(&vector.identity));
      assert_eq!(
        hex(&identity),
        vector.identity_hex,
        "{} identity",
        vector.name
      );
      assert_eq!(
        scope(&identity, &vector.config_fingerprint),
        vector.scope_id,
        "{} scope",
        vector.name
      );
    }
    assert_eq!(vectors[0].scope_id, vectors[1].scope_id);
    assert_ne!(vectors[0].scope_id, vectors[2].scope_id);
  }

  #[test]
  fn real_workspace_identity_smoke_test_is_machine_local_only() {
    let identity = workspace_identity(env!("CARGO_MANIFEST_DIR")).expect("workspace identity");
    assert!(identity.starts_with(b"workspace-file-id-v1\0"));
  }
}

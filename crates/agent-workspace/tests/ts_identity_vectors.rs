use agent_workspace::workspace_identity;
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

fn encode_inode(device_id: u64, inode_number: u64) -> Vec<u8> {
  let mut bytes = b"workspace-file-id-v1\0".to_vec();
  bytes.push(1);
  bytes.extend_from_slice(&device_id.to_be_bytes());
  bytes.extend_from_slice(&inode_number.to_be_bytes());
  bytes
}

fn encode_low_res(volume_serial_number: u32, file_index: u64) -> Vec<u8> {
  let mut bytes = b"workspace-file-id-v1\0".to_vec();
  bytes.push(2);
  bytes.extend_from_slice(&volume_serial_number.to_be_bytes());
  bytes.extend_from_slice(&file_index.to_be_bytes());
  bytes
}

fn encode_high_res(volume_serial_number: u64, file_id: u128) -> Vec<u8> {
  let mut bytes = b"workspace-file-id-v1\0".to_vec();
  bytes.push(3);
  bytes.extend_from_slice(&volume_serial_number.to_be_bytes());
  bytes.extend_from_slice(&file_id.to_be_bytes());
  bytes
}

fn hex(bytes: &[u8]) -> String {
  bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_hex_u64(value: &str) -> u64 {
  u64::from_str_radix(value.trim_start_matches("0x"), 16).expect("u64 fixture field")
}

fn parse_hex_u128(value: &str) -> u128 {
  u128::from_str_radix(value.trim_start_matches("0x"), 16).expect("u128 fixture field")
}

fn encode(identity: &Identity) -> Vec<u8> {
  match identity {
    Identity::Inode {
      device_id,
      inode_number,
    } => encode_inode(parse_hex_u64(device_id), parse_hex_u64(inode_number)),
    Identity::LowRes {
      volume_serial_number,
      file_index,
    } => encode_low_res(
      u32::try_from(parse_hex_u64(volume_serial_number)).expect("u32 fixture field"),
      parse_hex_u64(file_index),
    ),
    Identity::HighRes {
      volume_serial_number,
      file_id,
    } => encode_high_res(parse_hex_u64(volume_serial_number), parse_hex_u128(file_id)),
  }
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
fn committed_typescript_vectors_match_the_rust_identity_encoding() {
  let fixture = include_str!(
    "../../../packages/local-control-plane/tests/fixtures/rust-workspace-identity-vectors.json"
  );
  let vectors: Vec<Vector> = serde_json::from_str(fixture).expect("valid identity fixture");
  for vector in &vectors {
    let identity = encode(&vector.identity);
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

use agent_protocol::v1::{
  HandshakeResponse, ModelCatalog, ModelPolicy, ModelSetupSnapshot, RequestMetadata,
  RuntimeSnapshot,
};
use prost::Message;

#[test]
fn request_metadata_field_numbers_are_stable() {
  let encoded = RequestMetadata {
    protocol_version: 1,
    client_instance_id: "tui".into(),
    command_id: "cmd".into(),
    expected_server_instance_id: String::new(),
  }
  .encode_to_vec();

  assert_eq!(
    encoded,
    vec![
      0x08, 0x01, 0x12, 0x03, b't', b'u', b'i', 0x1a, 0x03, b'c', b'm', b'd'
    ]
  );
}

#[test]
fn handshake_scope_uses_additive_field_five() {
  let scope = "a".repeat(64);
  let encoded = HandshakeResponse {
    protocol_version: 1,
    server_version: String::new(),
    server_instance_id: String::new(),
    capabilities: Vec::new(),
    scope_fingerprint: scope.clone(),
  }
  .encode_to_vec();

  let mut expected = vec![0x08, 0x01, 0x2a, 0x40];
  expected.extend_from_slice(scope.as_bytes());
  assert_eq!(encoded, expected);
}

#[test]
fn pre_scope_handshake_still_decodes() {
  let encoded = [
    0x08, 0x01, 0x12, 0x03, b'0', b'.', b'1', 0x1a, 0x06, b'd', b'a', b'e', b'm', b'o', b'n',
  ];

  let decoded = HandshakeResponse::decode(encoded.as_slice()).unwrap();

  assert_eq!(decoded.protocol_version, 1);
  assert_eq!(decoded.server_version, "0.1");
  assert_eq!(decoded.server_instance_id, "daemon");
  assert!(decoded.scope_fingerprint.is_empty());
}

#[test]
fn runtime_snapshot_model_setup_uses_additive_field_eleven() {
  let encoded = RuntimeSnapshot {
    model_setup: Some(ModelSetupSnapshot::default()),
    ..RuntimeSnapshot::default()
  }
  .encode_to_vec();

  assert_eq!(encoded, vec![0x5a, 0x00]);
}

#[test]
fn pre_model_setup_snapshot_still_decodes() {
  let encoded = [0x08, 0x01, 0x10, 0x07, 0x18, 0x03];

  let decoded = RuntimeSnapshot::decode(encoded.as_slice()).unwrap();

  assert_eq!(decoded.protocol_version, 1);
  assert_eq!(decoded.revision, 7);
  assert!(decoded.model_setup.is_none());
}

#[test]
fn runtime_snapshot_model_catalog_and_policy_use_additive_fields() {
  let catalog = RuntimeSnapshot {
    model_catalog: Some(ModelCatalog::default()),
    ..RuntimeSnapshot::default()
  }
  .encode_to_vec();
  let policy = RuntimeSnapshot {
    model_policy: Some(ModelPolicy::default()),
    ..RuntimeSnapshot::default()
  }
  .encode_to_vec();

  assert_eq!(catalog, vec![0x62, 0x00]);
  assert_eq!(policy, vec![0x6a, 0x00]);
}

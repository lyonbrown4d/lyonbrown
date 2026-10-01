use agent_protocol::v1::RequestMetadata;
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

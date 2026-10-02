use agent_local::{DiscoveryDescriptor, DiscoveryError, ScopeFingerprint, SecretToken, Transport};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use uuid::Uuid;

fn valid_descriptor() -> DiscoveryDescriptor {
  DiscoveryDescriptor::new(
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 43117),
    42,
    Uuid::new_v4(),
    SecretToken::generate().expect("token generation"),
    ScopeFingerprint::parse(&"ab".repeat(32)).expect("scope fingerprint"),
  )
  .expect("valid descriptor")
}

#[test]
fn descriptor_round_trips_with_a_strict_versioned_schema() {
  let descriptor = valid_descriptor();
  let encoded = descriptor.to_json().expect("serialize descriptor");
  let decoded = DiscoveryDescriptor::from_json(&encoded).expect("parse descriptor");

  assert_eq!(decoded.schema_version(), 1);
  assert_eq!(decoded.transport(), Transport::GrpcTcp);
  assert_eq!(decoded.address().to_string(), "127.0.0.1:43117");
  assert_eq!(decoded.pid(), 42);
  assert_eq!(
    decoded.server_instance_id(),
    descriptor.server_instance_id()
  );
  assert_eq!(decoded.scope_fingerprint(), descriptor.scope_fingerprint());
}

#[test]
fn descriptor_rejects_unknown_fields_and_oversized_input() {
  let encoded = valid_descriptor().to_json().expect("serialize descriptor");
  let mut value: serde_json::Value = serde_json::from_slice(&encoded).expect("json object");
  value["unexpected"] = serde_json::json!(true);
  let unknown = serde_json::to_vec(&value).expect("json encoding");

  assert!(matches!(
    DiscoveryDescriptor::from_json(&unknown),
    Err(DiscoveryError::InvalidDescriptor(_))
  ));
  assert!(matches!(
    DiscoveryDescriptor::from_json(&vec![b' '; 4097]),
    Err(DiscoveryError::DescriptorTooLarge { .. })
  ));
}

#[test]
fn descriptor_accepts_only_numeric_loopback_with_nonzero_port() {
  let token = SecretToken::generate().expect("token generation");
  let scope = ScopeFingerprint::parse(&"cd".repeat(32)).expect("scope fingerprint");
  let id = Uuid::new_v4();

  let unspecified = DiscoveryDescriptor::new(
    "0.0.0.0:9000".parse().expect("socket address"),
    10,
    id,
    token.clone(),
    scope.clone(),
  );
  let zero_port = DiscoveryDescriptor::new(
    "127.0.0.1:0".parse().expect("socket address"),
    10,
    id,
    token,
    scope,
  );

  assert!(matches!(
    unspecified,
    Err(DiscoveryError::NonLoopbackAddress(_))
  ));
  assert!(matches!(zero_port, Err(DiscoveryError::ZeroPort)));
}

#[test]
fn scope_fingerprint_requires_canonical_sha256_hex() {
  assert!(ScopeFingerprint::parse(&"01".repeat(32)).is_ok());
  assert!(ScopeFingerprint::parse(&"A1".repeat(32)).is_err());
  assert!(ScopeFingerprint::parse("abcd").is_err());
}

#[test]
fn descriptor_rejects_a_nil_server_instance_id() {
  let result = DiscoveryDescriptor::new(
    "[::1]:43117".parse().expect("socket address"),
    42,
    Uuid::nil(),
    SecretToken::generate().expect("token generation"),
    ScopeFingerprint::parse(&"12".repeat(32)).expect("scope fingerprint"),
  );

  assert!(matches!(result, Err(DiscoveryError::InvalidDescriptor(_))));
}

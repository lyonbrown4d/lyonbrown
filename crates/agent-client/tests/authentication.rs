use agent_client::{AuthenticatedTarget, AuthorizationInterceptor};
use agent_local::{AUTHORIZATION_METADATA_KEY, DiscoveryDescriptor, ScopeFingerprint, SecretToken};
use std::net::{Ipv4Addr, SocketAddr};
use tonic::Request;
use tonic::service::Interceptor;
use uuid::Uuid;

fn descriptor(token: SecretToken) -> DiscoveryDescriptor {
  DiscoveryDescriptor::new(
    SocketAddr::from((Ipv4Addr::LOCALHOST, 7788)),
    42,
    Uuid::new_v4(),
    token,
    ScopeFingerprint::parse(&"ab".repeat(32)).expect("scope"),
  )
  .expect("validated discovery descriptor")
}

#[test]
fn authenticated_target_debug_never_exposes_credential() {
  let token = SecretToken::generate().expect("token");
  let secret = token.expose_for_transport();
  let target = AuthenticatedTarget::from_descriptor(&descriptor(token));

  let debug = format!("{target:?}");

  assert!(!debug.contains(&secret));
  assert!(debug.contains("http://127.0.0.1:7788"));
}

#[test]
fn authenticated_target_derives_its_endpoint_from_validated_discovery() {
  let target =
    AuthenticatedTarget::from_descriptor(&descriptor(SecretToken::generate().expect("token")));

  assert_eq!(target.endpoint(), "http://127.0.0.1:7788");
}

#[test]
fn authorization_interceptor_injects_one_sensitive_bearer_value() {
  let token = SecretToken::generate().expect("token");
  let expected = format!("Bearer {}", token.expose_for_transport());
  let mut interceptor = AuthorizationInterceptor::new(&token);

  let request = interceptor.call(Request::new(())).expect("intercept");
  let values = request
    .metadata()
    .get_all(AUTHORIZATION_METADATA_KEY)
    .iter()
    .collect::<Vec<_>>();

  assert_eq!(values.len(), 1);
  assert_eq!(values[0].to_str().expect("ASCII metadata"), expected);
  assert!(values[0].is_sensitive());
  assert!(!format!("{interceptor:?}").contains(&token.expose_for_transport()));
}

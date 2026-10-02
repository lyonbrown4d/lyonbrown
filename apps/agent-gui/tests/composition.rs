use agent_client::{AuthenticatedTarget, NativeAgentClient, RemoteClientOptions};
use agent_gui::composition::CompositionRoot;
use agent_gui::state::ConnectionState;
use agent_local::{DiscoveryDescriptor, ScopeFingerprint, SecretToken};
use uuid::Uuid;

#[test]
fn unavailable_composition_requires_an_explicit_connection_error() {
  let root = CompositionRoot::unavailable("agentd startup failed").unwrap();

  root.validate().unwrap();
  let cockpit = root.create_cockpit().unwrap();
  assert!(!cockpit.has_live_client());
  assert_eq!(cockpit.state().connection(), ConnectionState::Unavailable);
  assert_eq!(
    cockpit.state().last_connection_error(),
    Some("agentd startup failed")
  );
}

#[test]
fn composition_root_injects_a_native_client_without_claiming_connection() {
  let descriptor = DiscoveryDescriptor::new(
    "127.0.0.1:9".parse().unwrap(),
    1,
    Uuid::new_v4(),
    SecretToken::generate().unwrap(),
    ScopeFingerprint::parse(&"ab".repeat(32)).unwrap(),
  )
  .unwrap();
  let client = NativeAgentClient::start(
    AuthenticatedTarget::from_descriptor(&descriptor),
    "gui-composition-test",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();
  let root = CompositionRoot::with_native_client(client).unwrap();

  let cockpit = root.create_cockpit().unwrap();

  assert!(cockpit.has_live_client());
  assert_eq!(cockpit.state().connection(), ConnectionState::Disconnected);
}

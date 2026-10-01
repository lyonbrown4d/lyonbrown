use agent_client::{NativeAgentClient, RemoteClientOptions};
use agent_gui::composition::CompositionRoot;
use agent_gui::state::{CockpitState, ConnectionState};

#[test]
fn composition_root_validates_and_creates_cockpit() {
  let root = CompositionRoot::new().unwrap();

  root.validate().unwrap();
  let cockpit = root.create_cockpit().unwrap();
  assert!(!cockpit.has_live_client());
}

#[test]
fn composition_root_accepts_an_initial_offline_state() {
  let mut state = CockpitState::default();
  state.apply_connection(
    ConnectionState::Unavailable,
    Some("agentd is unavailable".into()),
  );
  let root = CompositionRoot::with_initial_state(state).unwrap();

  let cockpit = root.create_cockpit().unwrap();

  assert_eq!(cockpit.state().connection(), ConnectionState::Unavailable);
  assert_eq!(
    cockpit.state().last_connection_error(),
    Some("agentd is unavailable")
  );
}

#[test]
fn composition_root_injects_a_native_client_without_claiming_connection() {
  let client = NativeAgentClient::start(
    "http://127.0.0.1:9",
    "gui-composition-test",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();
  let root = CompositionRoot::with_native_client(client).unwrap();

  let cockpit = root.create_cockpit().unwrap();

  assert!(cockpit.has_live_client());
  assert_eq!(cockpit.state().connection(), ConnectionState::Disconnected);
}

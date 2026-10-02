#[path = "support/epoch_server.rs"]
mod epoch_server;

use std::time::Duration;

use agent_app::{AgentClientError, AvailabilityKind};
use agent_client::{
  ConnectionState as ClientConnectionState, NativeAgentClient, RemoteClientOptions,
};
use agent_gui::state::{CockpitState, ConnectionState};

use epoch_server::EpochServer;

#[tokio::test]
async fn snapshot_epoch_a_submission_is_not_delivered_to_native_epoch_b() {
  let epoch_b = EpochServer::spawn().await;
  let client = NativeAgentClient::start(
    epoch_b.target.clone(),
    "gui-submission-epoch-test",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();
  wait_for_epoch(&client, &epoch_b.server_instance_id).await;

  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);
  assert!(state.apply_snapshot_revision("daemon-a", 7));
  let dispatch = state.begin_submission("must-not-reach-b").unwrap();

  assert_eq!(dispatch.expected_server_instance_id, "daemon-a");
  let error = client
    .dispatch_for_server(dispatch.expected_server_instance_id, dispatch.command)
    .await
    .unwrap_err();
  assert!(matches!(
    error,
    AgentClientError::Unavailable {
      kind: AvailabilityKind::StaleEpoch,
      ..
    }
  ));
  assert!(epoch_b.commands.lock().unwrap().is_empty());
}

async fn wait_for_epoch(client: &NativeAgentClient, expected: &str) {
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if matches!(
        client.remote_state().connection,
        ClientConnectionState::Connected { server_instance_id } if server_instance_id == expected
      ) {
        break;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

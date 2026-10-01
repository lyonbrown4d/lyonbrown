use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use agent_app::{
  AgentClientError, AvailabilityKind, ProtocolErrorKind, RuntimeSnapshot as AppSnapshot,
};
use agent_protocol::PROTOCOL_VERSION;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tonic::Streaming;

use super::{
  ConnectionState, RemoteClientOptions, RemoteState, VersionedRemoteSnapshot, status_error,
};
use crate::{AgentClient, DecodedSnapshot, decode_snapshot};

pub(super) struct Established {
  pub client: AgentClient,
  pub events: Streaming<agent_protocol::v1::RuntimeEvent>,
  pub snapshot: DecodedSnapshot,
  pub server_instance_id: String,
}

pub(super) struct DriverContext {
  pub endpoint: String,
  pub client_instance_id: String,
  pub options: RemoteClientOptions,
  pub snapshots: watch::Sender<AppSnapshot>,
  pub remote_snapshots: watch::Sender<VersionedRemoteSnapshot>,
  pub connection: watch::Sender<ConnectionState>,
  pub remote_state: watch::Sender<RemoteState>,
  pub revision: Arc<AtomicU64>,
  pub cancellation: CancellationToken,
}

pub(super) async fn establish(
  endpoint: &str,
  client_instance_id: &str,
  options: &RemoteClientOptions,
) -> Result<Established, AgentClientError> {
  let connect = AgentClient::connect_with_timeouts(
    endpoint,
    client_instance_id.to_owned(),
    options.connect_timeout,
    options.rpc_timeout,
  );
  let mut client = tokio::time::timeout(options.connect_timeout, connect)
    .await
    .map_err(|_| AgentClientError::unavailable(AvailabilityKind::Timeout, "connect timeout"))?
    .map_err(|error| {
      AgentClientError::unavailable(AvailabilityKind::Disconnected, error.to_string())
    })?;
  let handshake = client
    .handshake("agent-client", env!("CARGO_PKG_VERSION"))
    .await
    .map_err(handshake_status_error)?;
  if handshake.protocol_version != PROTOCOL_VERSION {
    return Err(AgentClientError::protocol(
      ProtocolErrorKind::IncompatibleVersion,
      format!(
        "server protocol version {}, client supports {PROTOCOL_VERSION}",
        handshake.protocol_version
      ),
    ));
  }
  let snapshot = client
    .get_snapshot()
    .await
    .map_err(status_error)
    .and_then(|wire| {
      decode_snapshot(wire).map_err(|error| {
        AgentClientError::protocol(ProtocolErrorKind::InvalidSnapshot, error.to_string())
      })
    })?;
  let events = client
    .subscribe_events(snapshot.revision)
    .await
    .map_err(status_error)?;
  Ok(Established {
    client,
    events,
    snapshot,
    server_instance_id: handshake.server_instance_id,
  })
}

pub(super) async fn run_driver(mut established: Established, context: DriverContext) {
  let mut server_instance_id = established.server_instance_id.clone();
  loop {
    tokio::select! {
      _ = context.cancellation.cancelled() => return,
      event = established.events.message() => match event {
        Ok(Some(event)) => {
          if let Err(error) = apply_event(event, &context) {
            publish_connection(ConnectionState::Failed {
              server_instance_id: Some(server_instance_id),
              error,
            }, &context);
            return;
          }
          continue;
        }
        Ok(None) => {}
        Err(_) => {}
      }
    }
    let mut attempt = 1;
    loop {
      let error = AgentClientError::unavailable(
        AvailabilityKind::Disconnected,
        "runtime event stream disconnected",
      );
      publish_connection(
        ConnectionState::Reconnecting {
          server_instance_id: Some(server_instance_id.clone()),
          attempt,
          error,
        },
        &context,
      );
      let delay = bounded_backoff(
        context.options.reconnect_min_delay,
        context.options.reconnect_max_delay,
        attempt - 1,
      );
      tokio::select! {
        _ = context.cancellation.cancelled() => return,
        _ = tokio::time::sleep(delay) => {}
      }
      let reconnect = tokio::select! {
        _ = context.cancellation.cancelled() => return,
        result = establish(
          &context.endpoint,
          &context.client_instance_id,
          &context.options,
        ) => result,
      };
      match reconnect {
        Ok(reconnected) => {
          server_instance_id = reconnected.server_instance_id.clone();
          apply_snapshot(
            reconnected.snapshot.clone(),
            &server_instance_id,
            true,
            &context,
          );
          established = reconnected;
          break;
        }
        Err(error @ AgentClientError::Protocol { .. }) => {
          publish_connection(
            ConnectionState::Failed {
              server_instance_id: Some(server_instance_id),
              error,
            },
            &context,
          );
          return;
        }
        Err(_) => attempt = attempt.saturating_add(1),
      }
    }
  }
}

fn apply_event(
  event: agent_protocol::v1::RuntimeEvent,
  context: &DriverContext,
) -> Result<(), AgentClientError> {
  if event.protocol_version != PROTOCOL_VERSION {
    return Err(AgentClientError::protocol(
      ProtocolErrorKind::IncompatibleVersion,
      "event protocol version is incompatible",
    ));
  }
  let wire = event.snapshot.ok_or_else(|| {
    AgentClientError::protocol(
      ProtocolErrorKind::InvalidSnapshot,
      "event snapshot is missing",
    )
  })?;
  if event.revision != wire.revision {
    return Err(AgentClientError::protocol(
      ProtocolErrorKind::InvalidSnapshot,
      format!(
        "event revision {} does not match snapshot revision {}",
        event.revision, wire.revision
      ),
    ));
  }
  let decoded = decode_snapshot(wire).map_err(|error| {
    AgentClientError::protocol(ProtocolErrorKind::InvalidSnapshot, error.to_string())
  })?;
  let server_instance_id = match context.connection.borrow().clone() {
    ConnectionState::Connected { server_instance_id } => server_instance_id,
    _ => return Ok(()),
  };
  apply_snapshot(decoded, &server_instance_id, false, context);
  Ok(())
}

fn apply_snapshot(
  snapshot: DecodedSnapshot,
  server_instance_id: &str,
  resync: bool,
  context: &DriverContext,
) {
  let current = context.revision.load(Ordering::Acquire);
  if resync || snapshot.revision > current {
    let connection = ConnectionState::Connected {
      server_instance_id: server_instance_id.to_owned(),
    };
    let remote_snapshot = VersionedRemoteSnapshot {
      server_instance_id: server_instance_id.to_owned(),
      revision: snapshot.revision,
      snapshot: snapshot.snapshot.clone(),
    };
    context.revision.store(snapshot.revision, Ordering::Release);
    context.remote_state.send_replace(RemoteState {
      connection: connection.clone(),
      snapshot: Some(remote_snapshot.clone()),
    });
    if resync {
      context.connection.send_replace(connection);
    }
    context.remote_snapshots.send_replace(remote_snapshot);
    context.snapshots.send_replace(snapshot.snapshot);
  }
}

fn publish_connection(state: ConnectionState, context: &DriverContext) {
  let snapshot = context.remote_state.borrow().snapshot.clone();
  context.remote_state.send_replace(RemoteState {
    connection: state.clone(),
    snapshot,
  });
  context.connection.send_replace(state);
}

fn handshake_status_error(status: tonic::Status) -> AgentClientError {
  if status.code() == tonic::Code::FailedPrecondition {
    AgentClientError::protocol(ProtocolErrorKind::IncompatibleVersion, status.message())
  } else {
    status_error(status)
  }
}

pub(super) fn bounded_backoff(minimum: Duration, maximum: Duration, attempt: u32) -> Duration {
  let factor = 1u32.checked_shl(attempt.min(31)).unwrap_or(u32::MAX);
  minimum.saturating_mul(factor).min(maximum)
}

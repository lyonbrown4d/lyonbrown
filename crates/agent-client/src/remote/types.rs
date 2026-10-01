use std::time::Duration;

use agent_app::{AgentClientError, RuntimeSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
  Connecting,
  Connected {
    server_instance_id: String,
  },
  Reconnecting {
    server_instance_id: Option<String>,
    attempt: u32,
    error: AgentClientError,
  },
  Failed {
    server_instance_id: Option<String>,
    error: AgentClientError,
  },
}

#[derive(Debug, Clone, PartialEq)]
pub struct VersionedRemoteSnapshot {
  pub server_instance_id: String,
  pub revision: u64,
  pub snapshot: RuntimeSnapshot,
}

/// Authoritative atomic GUI state; consumers should prefer this over split watches.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteState {
  pub connection: ConnectionState,
  pub snapshot: Option<VersionedRemoteSnapshot>,
}

#[derive(Debug, Clone)]
pub struct RemoteClientOptions {
  pub connect_timeout: Duration,
  pub rpc_timeout: Duration,
  pub reconnect_min_delay: Duration,
  pub reconnect_max_delay: Duration,
  pub command_attempts: u32,
}

impl Default for RemoteClientOptions {
  fn default() -> Self {
    Self {
      connect_timeout: Duration::from_secs(5),
      rpc_timeout: Duration::from_secs(30),
      reconnect_min_delay: Duration::from_millis(100),
      reconnect_max_delay: Duration::from_secs(5),
      command_attempts: 3,
    }
  }
}

impl RemoteClientOptions {
  #[doc(hidden)]
  pub fn for_tests() -> Self {
    Self {
      connect_timeout: Duration::from_secs(1),
      rpc_timeout: Duration::from_secs(1),
      reconnect_min_delay: Duration::from_millis(5),
      reconnect_max_delay: Duration::from_millis(25),
      command_attempts: 3,
    }
  }
}

use agent_client::AgentClient;
use agent_protocol::v1::{Approval, Event, Run, Session};

pub enum Update {
  Connected {
    client: Box<AgentClient>,
    sessions: Vec<Session>,
    endpoint: String,
    node_id: String,
  },
  Session(Session, bool),
  Runs(Vec<Run>),
  Started(Run),
  Event(Event),
  Run(Run),
  Approvals(Vec<Approval>),
  Recovery(String, Box<agent_protocol::v1::RunRecovery>),
  RecoveryError(String, String),
  Error(String),
}

pub fn public_error(error: impl std::fmt::Display) -> String {
  // Client errors describe transport/status; credential values are never attached.
  format!("{error}. Check the daemon address, authentication and daemon logs.")
}

pub fn local_token(data_dir: &std::path::Path) -> String {
  std::fs::read_to_string(data_dir.join("auth.token"))
    .unwrap_or_default()
    .trim()
    .to_string()
}

pub(crate) fn frame_not_ready(error: &anyhow::Error) -> bool {
  error.downcast_ref::<tonic::Status>().is_some_and(|status| {
    status.code() == tonic::Code::Unavailable && status.message() == "no newer browser frame"
  })
}

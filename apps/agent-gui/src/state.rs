#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConnectionState {
  #[default]
  Disconnected,
  Connecting,
  Connected,
  Reconnecting,
  Unavailable,
  Incompatible,
}

#[derive(Clone, Debug, Default)]
pub struct CockpitState {
  runtime_details_expanded: bool,
  connection: ConnectionState,
  server_instance_id: Option<String>,
  revision: Option<u64>,
  stale: bool,
  last_connection_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeDetailsPlacement {
  Hidden,
  Conversation,
  RuntimeRail,
}

impl RuntimeDetailsPlacement {
  pub fn for_layout(compact: bool, expanded: bool) -> Self {
    match (compact, expanded) {
      (_, false) => Self::Hidden,
      (true, true) => Self::Conversation,
      (false, true) => Self::RuntimeRail,
    }
  }
}

impl CockpitState {
  pub fn connection(&self) -> ConnectionState {
    self.connection
  }

  pub fn server_instance_id(&self) -> Option<&str> {
    self.server_instance_id.as_deref()
  }

  pub fn revision(&self) -> Option<u64> {
    self.revision
  }

  pub fn is_stale(&self) -> bool {
    self.stale
  }

  pub fn last_connection_error(&self) -> Option<&str> {
    self.last_connection_error.as_deref()
  }

  pub fn can_submit(&self) -> bool {
    self.connection == ConnectionState::Connected && self.revision.is_some() && !self.stale
  }

  pub fn apply_connection(&mut self, connection: ConnectionState, error: Option<String>) {
    self.connection = connection;
    if connection != ConnectionState::Connected && self.revision.is_some() {
      self.stale = true;
    }
    self.last_connection_error = match connection {
      ConnectionState::Reconnecting
      | ConnectionState::Unavailable
      | ConnectionState::Incompatible => error,
      ConnectionState::Disconnected | ConnectionState::Connecting | ConnectionState::Connected => {
        None
      }
    };
  }

  pub fn apply_snapshot_revision(&mut self, server_instance_id: &str, revision: u64) -> bool {
    let same_instance = self.server_instance_id.as_deref() == Some(server_instance_id);
    let accept = if same_instance {
      match self.revision {
        Some(current) if revision > current => true,
        Some(current) if revision == current => self.stale,
        Some(_) => false,
        None => true,
      }
    } else {
      true
    };
    if !accept {
      return false;
    }

    self.server_instance_id = Some(server_instance_id.to_owned());
    self.revision = Some(revision);
    self.stale = self.connection != ConnectionState::Connected;
    true
  }

  pub(crate) fn mark_snapshot_stale(&mut self) {
    self.stale = self.revision.is_some();
  }

  pub fn runtime_details_expanded(&self) -> bool {
    self.runtime_details_expanded
  }

  pub fn runtime_details_control_label(&self) -> &'static str {
    if self.runtime_details_expanded {
      "Hide details"
    } else {
      "Show details"
    }
  }

  pub fn toggle_runtime_details(&mut self) {
    self.runtime_details_expanded = !self.runtime_details_expanded;
  }
}

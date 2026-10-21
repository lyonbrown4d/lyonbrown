#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Connection {
  Disconnected(String),
  Connecting,
  Connected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
  pub id: String,
  pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
  pub role: String,
  pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
  pub id: String,
  pub description: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContextPanel {
  #[default]
  Closed,
  Help,
  Sessions,
  Tasks,
  Computer,
  Approval,
}

#[derive(Debug)]
pub struct App {
  pub panel: ContextPanel,
  pub connection: Connection,
  pub computer: crate::computer::ComputerStatus,
  pub computer_detail: String,
  pub computer_scroll: u16,
  pub sessions: Vec<Session>,
  pub selected: usize,
  pub messages: Vec<Message>,
  pub tasks: Vec<String>,
  pub active_run: Option<String>,
  pub streaming: Option<(String, String)>,
  pub approvals: Vec<Approval>,
  pub input: crate::composer::Composer,
  pub submitting: bool,
  pub notice: String,
  pub scroll: u16,
}

impl Default for App {
  fn default() -> Self {
    Self {
      panel: ContextPanel::Closed,
      connection: Connection::Disconnected("Not connected".into()),
      computer: if cfg!(feature = "native-computer") {
        crate::computer::ComputerStatus::Detached
      } else {
        crate::computer::ComputerStatus::Unavailable
      },
      computer_detail: String::new(),
      computer_scroll: 0,
      sessions: Vec::new(),
      selected: 0,
      messages: Vec::new(),
      tasks: Vec::new(),
      active_run: None,
      streaming: None,
      approvals: Vec::new(),
      input: crate::composer::Composer::default(),
      submitting: false,
      notice: String::new(),
      scroll: 0,
    }
  }
}

impl App {
  pub fn selected_session(&self) -> Option<&Session> {
    self.sessions.get(self.selected)
  }

  pub fn next_session(&mut self) {
    if !self.sessions.is_empty() {
      self.selected = (self.selected + 1) % self.sessions.len();
      self.messages.clear();
      self.tasks.clear();
      self.active_run = None;
      self.streaming = None;
      self.approvals.clear();
      self.scroll = 0;
    }
  }
}

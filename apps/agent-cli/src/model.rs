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

#[derive(Debug)]
pub struct App {
  pub connection: Connection,
  pub sessions: Vec<Session>,
  pub selected: usize,
  pub messages: Vec<Message>,
  pub tasks: Vec<String>,
  pub active_run: Option<String>,
  pub streaming: Option<(String, String)>,
  pub approvals: Vec<Approval>,
  pub input: String,
  pub submitting: bool,
  pub notice: String,
  pub scroll: u16,
}

impl Default for App {
  fn default() -> Self {
    Self {
            connection: Connection::Disconnected("Not connected".into()),
            sessions: Vec::new(),
            selected: 0,
            messages: Vec::new(),
            tasks: Vec::new(),
            active_run: None,
            streaming: None,
            approvals: Vec::new(),
            input: String::new(),
            submitting: false,
            notice: "Ctrl+C quit · Ctrl+N new · Ctrl+R reconnect · Tab session · Ctrl+X cancel · Ctrl+Y approve / Ctrl+E deny".into(),
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

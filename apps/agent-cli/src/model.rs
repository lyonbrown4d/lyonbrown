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
  Drafts,
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
  pub drafts: BTreeMap<String, String>,
  pub unbound_draft: String,
  pub persistence_warning: String,
  pub persistence_node_id: Option<String>,
  pub state_loaded: bool,
  pub task_rows: Vec<serde_json::Value>,
  pub task_cursor: String,
  pub task_selected: usize,
  pub task_detail: String,
  pub generation: u64,
  pub draft_detail: String,
  pub recovered_draft_from: Option<String>,
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
      drafts: BTreeMap::new(),
      unbound_draft: String::new(),
      persistence_warning: String::new(),
      persistence_node_id: None,
      state_loaded: true,
      task_rows: Vec::new(),
      task_cursor: String::new(),
      task_selected: 0,
      task_detail: String::new(),
      generation: 0,
      draft_detail: String::new(),
      recovered_draft_from: None,
    }
  }
}

impl App {
  pub fn selected_session(&self) -> Option<&Session> {
    self.sessions.get(self.selected)
  }

  pub fn next_session(&mut self) {
    if !self.sessions.is_empty() {
      self.save_current_draft();
      self.generation = self.generation.saturating_add(1);
      self.selected = (self.selected + 1) % self.sessions.len();
      self.restore_current_draft();
      self.messages.clear();
      self.tasks.clear();
      self.active_run = None;
      self.streaming = None;
      self.approvals.clear();
      self.task_rows.clear();
      self.task_cursor.clear();
      self.task_selected = 0;
      self.task_detail.clear();
      self.scroll = 0;
    }
  }

  pub fn save_current_draft(&mut self) {
    if self.recovered_draft_from.is_some() {
      self.unbound_draft = self.input.text();
    } else if let Some(session_id) = self.selected_session().map(|session| session.id.clone()) {
      let text = self.input.text();
      if text.is_empty() {
        self.drafts.remove(&session_id);
      } else {
        self.drafts.insert(session_id, text);
      }
    } else {
      self.unbound_draft = self.input.text();
    }
  }

  pub fn restore_current_draft(&mut self) {
    self.recovered_draft_from = None;
    let text = match self.selected_session() {
      Some(session) => self.drafts.get(&session.id).cloned().unwrap_or_default(),
      None => self.unbound_draft.clone(),
    };
    self.input = crate::composer::Composer::from(text.as_str());
  }

  pub fn client_state(&mut self) -> crate::local_state::ClientState {
    self.save_current_draft();
    crate::local_state::ClientState {
      selected_session: self.selected_session().map(|session| session.id.clone()),
      drafts: self.drafts.clone(),
      unbound_draft: self.unbound_draft.clone(),
      panel: self.panel.into(),
      transcript_scroll: self.scroll,
      context_scroll: self.computer_scroll,
    }
  }
}
use std::collections::BTreeMap;

use agent_core::AgentEvent;
use tokio::sync::broadcast;

pub struct EventBus {
  sender: broadcast::Sender<AgentEvent>,
  log: Vec<AgentEvent>,
}

impl EventBus {
  pub fn new(capacity: usize) -> Self {
    let (sender, _) = broadcast::channel(capacity.max(1));
    Self {
      sender,
      log: Vec::new(),
    }
  }

  pub fn emit(&mut self, event: AgentEvent) {
    self.log.push(event.clone());
    let _ = self.sender.send(event);
  }

  pub fn with_log(log: Vec<AgentEvent>) -> Self {
    Self {
      log,
      ..Self::default()
    }
  }

  pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
    self.sender.subscribe()
  }

  pub fn log(&self) -> &[AgentEvent] {
    &self.log
  }
}

impl Default for EventBus {
  fn default() -> Self {
    Self::new(256)
  }
}

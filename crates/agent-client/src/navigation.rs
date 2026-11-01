//! UI-independent navigation shared by interactive clients.
//! Contexts contain presentation state only, never daemon authority or credentials.

const HISTORY_LIMIT: usize = 32;

#[derive(Clone, Debug)]
pub struct Navigation<T> {
  current: T,
  past: Vec<T>,
  future: Vec<T>,
  revision: u64,
}

impl<T: Clone + PartialEq> Navigation<T> {
  pub fn new(current: T) -> Self {
    Self {
      current,
      past: Vec::new(),
      future: Vec::new(),
      revision: 0,
    }
  }

  pub fn current(&self) -> &T {
    &self.current
  }

  /// Capture before asynchronous presentation work; compare before changing routes.
  pub fn revision(&self) -> u64 {
    self.revision
  }

  /// Supersede pending presentation work without changing location or history.
  pub fn invalidate(&mut self) {
    self.changed();
  }

  pub fn navigate(&mut self, next: T) {
    if self.current == next {
      return;
    }
    if self.past.len() == HISTORY_LIMIT {
      self.past.remove(0);
    }
    self.past.push(std::mem::replace(&mut self.current, next));
    self.future.clear();
    self.changed();
  }

  pub fn back(&mut self) -> bool {
    let Some(previous) = self.past.pop() else {
      return false;
    };
    self
      .future
      .push(std::mem::replace(&mut self.current, previous));
    self.changed();
    true
  }

  pub fn forward(&mut self) -> bool {
    let Some(next) = self.future.pop() else {
      return false;
    };
    self.past.push(std::mem::replace(&mut self.current, next));
    self.changed();
    true
  }

  /// Rebind to a new connection/session and invalidate all earlier navigation intents.
  pub fn reset(&mut self, current: T) {
    self.current = current;
    self.past.clear();
    self.future.clear();
    self.changed();
  }

  /// Update local scroll/focus context without recording a navigation event.
  pub fn update_current(&mut self, update: impl FnOnce(&mut T)) {
    update(&mut self.current);
  }

  fn changed(&mut self) {
    self.revision = self.revision.saturating_add(1);
  }
}

impl<T: Clone + PartialEq + Default> Default for Navigation<T> {
  fn default() -> Self {
    Self::new(T::default())
  }
}

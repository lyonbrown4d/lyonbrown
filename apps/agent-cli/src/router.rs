//! Client navigation only; routes never own daemon sessions or execution authority.
use crate::model::ContextPanel;
use agent_client::navigation::Navigation;

#[derive(Debug)]
pub struct Router {
  navigation: Navigation<RouteContext>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RouteContext {
  panel: ContextPanel,
  scroll: u16,
}

impl Default for Router {
  fn default() -> Self {
    Self::from(ContextPanel::Closed)
  }
}

impl From<ContextPanel> for Router {
  fn from(current: ContextPanel) -> Self {
    Self {
      navigation: Navigation::new(RouteContext {
        panel: current,
        scroll: 0,
      }),
    }
  }
}

impl Router {
  pub fn current(&self) -> ContextPanel {
    self.navigation.current().panel
  }

  pub fn navigate(&mut self, route: ContextPanel) {
    if route == self.current() {
      return;
    }
    if route == ContextPanel::Closed {
      self.close();
      return;
    }
    self.navigation.navigate(RouteContext {
      panel: route,
      scroll: 0,
    });
  }

  pub fn back(&mut self) {
    if !self.navigation.back() {
      // Esc also revokes a pending presentation request when already at the root.
      self.restore(ContextPanel::Closed);
    }
  }

  /// Restore a persisted route without retaining history from another connection.
  pub fn restore(&mut self, route: ContextPanel) {
    self.navigation.reset(RouteContext {
      panel: route,
      scroll: 0,
    });
  }

  pub fn toggle(&mut self, route: ContextPanel) {
    if self.current() == route {
      self.back();
    } else {
      self.navigate(route);
    }
  }

  pub fn close(&mut self) {
    self.restore(ContextPanel::Closed);
  }

  pub fn scroll(&self) -> u16 {
    self.navigation.current().scroll
  }

  pub fn set_scroll(&mut self, scroll: u16) {
    self
      .navigation
      .update_current(|current| current.scroll = scroll);
  }

  pub fn revision(&self) -> u64 {
    self.navigation.revision()
  }

  /// A newer resource command supersedes any queued presentation result on this page.
  pub fn begin_request(&mut self) -> u64 {
    self.navigation.invalidate();
    self.navigation.revision()
  }

  pub fn reset_scroll(&mut self, panel: ContextPanel) {
    if self.current() == panel {
      self.set_scroll(0);
    }
  }

  /// A new session/connection cannot inherit a return stack or transient resource details.
  pub fn rebind(&mut self) {
    let route = match self.current() {
      ContextPanel::Resource => ContextPanel::Closed,
      other => other,
    };
    self.restore(route);
  }
}

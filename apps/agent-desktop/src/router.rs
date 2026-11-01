//! Per-window presentation navigation; daemon sessions remain owned by agentd.
use agent_client::navigation::Navigation;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Route {
  #[default]
  Chat,
  Settings(SettingsSection),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsSection {
  #[default]
  Connection,
  Models,
  Providers,
  Skills,
  Status,
  Appearance,
}

impl SettingsSection {
  pub const ALL: [Self; 6] = [
    Self::Connection,
    Self::Models,
    Self::Providers,
    Self::Skills,
    Self::Status,
    Self::Appearance,
  ];

  pub fn title(self) -> &'static str {
    match self {
      Self::Connection => "Connection",
      Self::Models => "Models",
      Self::Providers => "Providers",
      Self::Skills => "Skills",
      Self::Status => "Status",
      Self::Appearance => "Appearance",
    }
  }
}

/// Bounded history is local to a Desktop entity, including separate native windows.
#[derive(Debug)]
pub struct Router {
  navigation: Navigation<Route>,
}

impl Default for Router {
  fn default() -> Self {
    Self {
      navigation: Navigation::new(Route::Chat),
    }
  }
}

impl Router {
  pub fn current(&self) -> Route {
    *self.navigation.current()
  }

  pub fn is_settings(&self) -> bool {
    matches!(self.current(), Route::Settings(_))
  }

  pub fn navigate(&mut self, route: Route) {
    self.navigation.navigate(route);
  }

  pub fn back(&mut self) -> bool {
    self.navigation.back()
  }

  pub fn forward(&mut self) -> bool {
    self.navigation.forward()
  }

  /// The persistent return affordance exits the whole settings subtree.
  pub fn return_to_chat(&mut self) {
    self.navigate(Route::Chat);
  }
}

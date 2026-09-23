#[derive(Debug, Default)]
pub struct CockpitState {
  runtime_details_expanded: bool,
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

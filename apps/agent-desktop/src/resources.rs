//! Presentation and authenticated command adapters for daemon-owned resources.
mod context_panel;
mod dispatch;
mod hub;
mod hub_view;
mod memory;
mod models;
mod profile_editor;
mod providers;
mod skills;
mod state;
mod status;
mod workflow;

use agent_client::AgentClient;
use gpui_kit::*;
pub(crate) use state::ResourcePane;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResourceSection {
  Models,
  Providers,
  Skills,
  Status,
}

pub(crate) struct ResourcePanels {
  pub(crate) models: Entity<ResourcePane>,
  pub(crate) providers: Entity<ResourcePane>,
  pub(crate) skills: Entity<ResourcePane>,
  pub(crate) status: Entity<ResourcePane>,
  pub(crate) memory: Entity<memory::MemoryPanel>,
  pub(crate) workflow: Entity<workflow::WorkflowPane>,
}
impl ResourcePanels {
  pub(crate) fn new(
    runtime: Arc<tokio::runtime::Runtime>,
    window: &mut Window,
    cx: &mut App,
  ) -> Self {
    let mut pane = |section| cx.new(|cx| ResourcePane::new(section, runtime.clone(), window, cx));
    Self {
      models: pane(ResourceSection::Models),
      providers: pane(ResourceSection::Providers),
      skills: pane(ResourceSection::Skills),
      status: pane(ResourceSection::Status),
      memory: cx.new(|cx| memory::MemoryPanel::new(runtime.clone(), window, cx)),
      workflow: cx.new(|cx| workflow::WorkflowPane::new(runtime.clone(), window, cx)),
    }
  }
  pub(crate) fn bind(
    &mut self,
    client: Option<AgentClient>,
    session: Option<String>,
    cx: &mut App,
  ) {
    for pane in [&self.models, &self.providers, &self.skills, &self.status] {
      pane.update(cx, |pane, cx| {
        pane.bind(client.clone(), session.clone(), cx)
      });
    }
    self.memory.update(cx, |panel, cx| {
      panel.bind(client.clone(), session.clone(), cx)
    });
    self
      .workflow
      .update(cx, |panel, cx| panel.bind(client, session, cx));
  }
  /// Called before replacing a connection, including a connected-to-connected transition.
  pub(crate) fn invalidate(&mut self, cx: &mut App) {
    self.bind(None, None, cx);
  }
  pub(crate) fn blocks_connection(&self, cx: &App) -> bool {
    self.workflow.read(cx).pending
      || self.memory.read(cx).pending
      || [&self.models, &self.providers, &self.skills, &self.status]
        .iter()
        .any(|pane| {
          pane
            .read(cx)
            .context_panel
            .as_ref()
            .is_some_and(|panel| panel.read(cx).pending)
            || pane.read(cx).pending_mutations > 0
            || pane
              .read(cx)
              .profile_editor
              .as_ref()
              .is_some_and(|editor| editor.read(cx).pending)
        })
  }
  pub(crate) fn view(&self, section: ResourceSection) -> AnyElement {
    match section {
      ResourceSection::Models => self.models.clone().into_any_element(),
      ResourceSection::Providers => self.providers.clone().into_any_element(),
      ResourceSection::Skills => self.skills.clone().into_any_element(),
      ResourceSection::Status => self.status.clone().into_any_element(),
    }
  }
}

#[cfg(test)]
#[path = "../tests/support/resource_generation.rs"]
mod generation_tests;

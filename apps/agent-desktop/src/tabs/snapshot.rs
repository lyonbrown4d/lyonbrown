//! Chat-tab snapshots and restoration: presentation identity for the tab bar,
//! the persisted tab DTOs, and applying a restored local-state payload.

use super::{ChatTabSnapshot, state::ChatState};
use crate::shell::Desktop;
use gpui_kit::*;

impl Desktop {
  pub fn chat_tab_snapshots(&self, _cx: &App) -> Vec<ChatTabSnapshot> {
    self
      .navigation
      .order
      .iter()
      .filter_map(|id| {
        let (session, run) = if *id == self.navigation.active {
          (&self.vm.selected, &self.vm.active_run)
        } else {
          let state = self.navigation.inactive.get(id)?;
          (&state.vm.selected, &state.vm.active_run)
        };
        let title = if self.navigation.unbound.contains_key(id) {
          "Unbound restored draft".into()
        } else {
          session
            .as_ref()
            .and_then(|id| self.sessions.iter().find(|session| &session.id == id))
            .map(|session| session.title.clone())
            .unwrap_or_else(|| "New chat".into())
        };
        Some(ChatTabSnapshot {
          id: *id,
          title,
          session_id: session.clone(),
          active_run: run.clone(),
          selected: *id == self.navigation.active,
        })
      })
      .collect()
  }

  pub(crate) fn persisted_tabs(&self, cx: &App) -> Vec<crate::local_state::TabState> {
    self
      .navigation
      .order
      .iter()
      .filter_map(|id| {
        if *id == self.navigation.active {
          Some(crate::local_state::TabState {
            id: *id,
            session_id: self.vm.selected.clone(),
            unbound_from: self.navigation.unbound.get(id).cloned(),
            composer: self.composer.read(cx).value().to_string(),
            workspace: self.workspace.read(cx).value().to_string(),
          })
        } else {
          let state = self.navigation.inactive.get(id)?;
          Some(crate::local_state::TabState {
            id: *id,
            session_id: state.vm.selected.clone(),
            unbound_from: self.navigation.unbound.get(id).cloned(),
            composer: state.composer.read(cx).value().to_string(),
            workspace: state.workspace.read(cx).value().to_string(),
          })
        }
      })
      .collect()
  }

  pub(crate) fn restore_tabs(
    &mut self,
    tabs: &[crate::local_state::TabState],
    active: u64,
    sessions: &std::collections::HashSet<String>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if tabs.is_empty() {
      return;
    }
    let active_tab = tabs.iter().find(|tab| tab.id == active).unwrap_or(&tabs[0]);
    self.navigation.inactive.clear();
    self.navigation.unbound.clear();
    self.navigation.order.clear();
    self.navigation.active = active_tab.id;
    self.navigation.next_id = tabs
      .iter()
      .map(|tab| tab.id)
      .max()
      .unwrap_or(0)
      .saturating_add(1)
      .max(1);
    for tab in tabs {
      self.navigation.order.push(tab.id);
      let selected = tab.session_id.clone().filter(|id| sessions.contains(id));
      let unbound_from = selected
        .is_none()
        .then(|| tab.unbound_from.clone().or_else(|| tab.session_id.clone()))
        .flatten();
      if let Some(original) = unbound_from.clone() {
        self.navigation.unbound.insert(tab.id, original);
      }
      if tab.id == active_tab.id {
        self.workspace.update(cx, |workspace, cx| {
          workspace.set_value(tab.workspace.clone(), window, cx)
        });
        self.composer.update(cx, |composer, cx| {
          composer.set_value(tab.composer.clone(), window, cx)
        });
        self.vm.selected = selected;
        self.vm.runs.clear();
        self.vm.active_run = None;
        self.vm.cursor = 0;
        self.vm.approvals.clear();
        self.vm.conversation.clear();
      } else {
        let mut state = ChatState::new(tab.workspace.clone(), self.client.is_some(), window, cx);
        state.vm.selected = selected;
        state.composer.update(cx, |composer, cx| {
          composer.set_value(tab.composer.clone(), window, cx)
        });
        self.navigation.inactive.insert(tab.id, state);
      }
    }
    self.bind_active_chat(window, cx);
  }
}

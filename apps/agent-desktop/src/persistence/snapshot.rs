use crate::local_state::DesktopState;
use crate::{persistence::worker::Command, shell::Desktop};
use gpui_kit::{component::ActiveTheme, *};

impl Desktop {
  pub(crate) fn queue_snapshot(&mut self, cx: &mut Context<Self>) {
    let (Some(scope), Some(sender)) = (&self.persistence_scope, &self.persistence_sender) else {
      return;
    };
    if !self.persistence_loaded || self.persistence_loading {
      return;
    }
    let state = DesktopState {
      tabs: self.persisted_tabs(cx),
      active_tab: self.navigation.active,
      show_sidebar: self.show_sidebar,
      show_workbench: self.show_workbench,
      dock: self.dock.read(cx).dump(cx),
      layout_sizes: self
        .main_layout
        .read(cx)
        .sizes()
        .iter()
        .map(|size| size.as_f32())
        .collect(),
      theme: cx.theme().mode,
      editor_drafts: self.tasks.read(cx).persisted_editor_drafts(cx),
    };
    if let Err(error) = state.validate() {
      self.vm.error = Some(format!(
        "Local desktop state was not saved because it cannot be safely restored: {error}"
      ));
      cx.notify();
      return;
    }
    let Ok(encoded) = serde_json::to_string(&state) else {
      return;
    };
    if self.last_enqueued.as_deref() == Some(encoded.as_str()) {
      return;
    }
    self.persistence_sequence = self.persistence_sequence.saturating_add(1);
    self.last_enqueued = Some(encoded);
    self.latest_state = Some(state.clone());
    if let Ok(mut flush) = self.flush_control.lock() {
      flush.sequence = self.persistence_sequence;
      flush.state = Some(state.clone());
    }
    if let Ok(mut latest) = self.latest_shared.lock() {
      *latest = Some((self.persistence_sequence, state));
    }
    let command = Command::Save {
      scope: scope.clone(),
      generation: self.persistence_generation,
      sequence: self.persistence_sequence,
    };
    let _ = sender.try_send(command);
  }
}

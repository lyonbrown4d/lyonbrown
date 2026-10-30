//! Restore the existing window's panels through the component library registry.
use std::{
  collections::HashMap,
  sync::{Arc, Weak},
};

use gpui_kit::component::dock::{
  BasePanel, BasePanelView, DockArea, Panel, PanelEvent, PanelId, PanelState, panel_handle,
  register_panel,
};
use gpui_kit::*;

#[derive(Default)]
struct Panels(HashMap<(EntityId, String), Weak<dyn BasePanelView>>);
impl Global for Panels {}

pub(super) fn register(
  area: &Entity<DockArea>,
  navigation: &[(&'static str, PanelId)],
  cx: &mut App,
) {
  if cx.try_global::<Panels>().is_none() {
    cx.set_global(Panels::default());
  }
  // Weak handles preserve per-window identity without keeping a closed window's
  // Workbench (and its local runtime attachments) alive through the registry.
  cx.global_mut::<Panels>()
    .0
    .retain(|_, panel| panel.strong_count() > 0);
  for &(name, id) in navigation {
    if let Some(panel) = area.read(cx).panel(id).cloned() {
      cx.global_mut::<Panels>()
        .0
        .insert((area.entity_id(), name.into()), Arc::downgrade(&panel));
    }
    register_panel(cx, name, |context, _, cx| {
      let key = (
        context.dock_area().entity_id(),
        context.state().panel_name.clone(),
      );
      if let Some(panel) = cx.global::<Panels>().0.get(&key).and_then(Weak::upgrade) {
        return panel;
      }
      // An unavailable panel remains inert and preserves its serialized payload.
      let state = context.state().clone();
      panel_handle(cx.new(|cx| UnavailablePanel {
        state,
        focus: cx.focus_handle(),
      }))
    });
  }
}

struct UnavailablePanel {
  state: PanelState,
  focus: FocusHandle,
}
impl BasePanel for UnavailablePanel {
  fn panel_name(&self) -> &'static str {
    "UnavailableWorkbenchPanel"
  }
  fn dump(&self, _: &App) -> PanelState {
    self.state.clone()
  }
}
impl Panel for UnavailablePanel {}
impl EventEmitter<PanelEvent> for UnavailablePanel {}
impl Focusable for UnavailablePanel {
  fn focus_handle(&self, _: &App) -> FocusHandle {
    self.focus.clone()
  }
}
impl Render for UnavailablePanel {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    div().child("This workbench panel is unavailable. Reopen the desktop to restore it.")
  }
}

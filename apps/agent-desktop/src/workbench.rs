mod browser;
mod drafts;
pub(crate) mod files;
mod ime;
mod input;
mod replies;
pub(crate) mod state;
mod tasks;
mod watch;

use gpui_kit::component::{
  ActiveTheme,
  dock::{BasePanel, DockArea, DockLayout, DockSkin, Panel, PanelEvent, PanelId, panel_handle},
  menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::*;
pub use state::Workbench;
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Kind {
  Browser,
  Computer,
  Files,
  Editor,
  Diff,
  Tasks,
}

struct WorkbenchPanel {
  title: &'static str,
  focus: FocusHandle,
  state: Entity<Workbench>,
  kind: Kind,
  dock: WeakEntity<DockArea>,
  panels: Arc<[(&'static str, PanelId)]>,
  _subscription: Subscription,
}

impl WorkbenchPanel {
  fn new(
    title: &'static str,
    kind: Kind,
    state: Entity<Workbench>,
    dock: WeakEntity<DockArea>,
    cx: &mut App,
  ) -> Entity<Self> {
    cx.new(|cx| Self {
      title,
      focus: cx.focus_handle(),
      _subscription: cx.observe(&state, |_, _, cx| cx.notify()),
      state,
      kind,
      dock,
      panels: Arc::default(),
    })
  }
}
impl EventEmitter<PanelEvent> for WorkbenchPanel {}
impl Focusable for WorkbenchPanel {
  fn focus_handle(&self, _: &App) -> FocusHandle {
    self.focus.clone()
  }
}
impl BasePanel for WorkbenchPanel {
  fn panel_name(&self) -> &'static str {
    self.title
  }
  fn closable(&self, _: &App) -> bool {
    false
  }
}
impl Panel for WorkbenchPanel {
  fn tab_name(&self, _: &App) -> Option<SharedString> {
    Some(self.title.into())
  }

  fn dropdown_menu(&mut self, menu: PopupMenu, _: &mut Window, _: &mut Context<Self>) -> PopupMenu {
    self.panels.iter().fold(menu, |menu, &(title, id)| {
      let dock = self.dock.clone();
      menu.item(
        PopupMenuItem::new(title)
          .checked(title == self.title)
          .on_click(move |_, window, cx| {
            let _ = dock.update(cx, |dock, cx| dock.select_panel(id, window, cx));
          }),
      )
    })
  }

  fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    div()
      .id(self.title)
      .aria_label(self.title)
      .child(self.title)
  }
}
impl Render for WorkbenchPanel {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let kind = self.kind;
    let content = self.state.update(cx, |state, cx| match kind {
      Kind::Browser => state.browser_view(window, cx),
      Kind::Computer => state.computer.clone().into_any_element(),
      Kind::Files => state.files_view(cx),
      Kind::Editor => state.editor_view(cx),
      Kind::Diff => state.diff_view(cx),
      Kind::Tasks => state.tasks_view(cx),
    });
    div()
      .size_full()
      .track_focus(&self.focus)
      .bg(cx.theme().background)
      .child(content)
  }
}

pub fn build(
  runtime: Arc<tokio::runtime::Runtime>,
  browser: fluxdi::Shared<crate::composition::BrowserPort>,
  computer: fluxdi::Shared<crate::composition::ComputerPort>,
  window: &mut Window,
  cx: &mut App,
) -> (Entity<DockArea>, Entity<Workbench>) {
  let state = cx.new(|cx| Workbench::new(runtime, browser, computer, window, cx));
  let (area, _) = DockSkin::dock_area("workbench", Some(1), window, cx);
  let panels = [
    ("Browser", Kind::Browser),
    ("Files", Kind::Files),
    ("Editor", Kind::Editor),
    ("Diff", Kind::Diff),
    ("Tasks", Kind::Tasks),
    ("Computer", Kind::Computer),
  ];
  let panels: Vec<_> = panels
    .into_iter()
    .map(|(title, kind)| WorkbenchPanel::new(title, kind, state.clone(), area.downgrade(), cx))
    .collect();
  let navigation: Arc<[_]> = panels
    .iter()
    .map(|panel| (panel.read(cx).title, PanelId::from(panel.entity_id())))
    .collect();
  let layout = panels
    .into_iter()
    .fold(DockLayout::tabs(), |layout, panel| {
      panel.update(cx, |panel, _| panel.panels = navigation.clone());
      layout.panel_view(panel_handle(panel), cx)
    });
  area.update(cx, |area, cx| area.set_center(layout, window, cx));
  (area, state)
}

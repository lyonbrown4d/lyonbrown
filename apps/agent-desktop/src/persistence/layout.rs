use crate::shell::Desktop;
use gpui_kit::*;

impl Desktop {
  pub(super) fn restore_layout_after_frame(
    &mut self,
    sizes: Vec<f32>,
    scope: String,
    generation: u64,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if sizes.is_empty() {
      return;
    }
    let desktop = cx.entity().downgrade();
    window.on_next_frame(move |window, cx| {
      let _ = desktop.update(cx, |desktop, cx| {
        if desktop.persistence_scope.as_deref() != Some(scope.as_str())
          || desktop.persistence_generation != generation
        {
          return;
        }
        // A later explicit visibility change takes precedence over restoration.
        if sizes.len() != 2 + usize::from(desktop.show_workbench) {
          return;
        }
        let layout = desktop.main_layout.read(cx);
        if layout.sizes().len() != sizes.len() || layout.container_size() <= px(0.) {
          desktop.restore_layout_after_frame(sizes, scope, generation, window, cx);
          cx.notify();
          return;
        }
        for (index, size) in sizes.into_iter().enumerate() {
          desktop.main_layout.update(cx, |layout, cx| {
            layout.resize_panel(index, px(size), window, cx)
          });
        }
        cx.notify();
      });
    });
    cx.notify();
  }
}

use agent_gui::app::Cockpit;
use gpui::{App, AppContext, Application, Bounds, WindowBounds, WindowOptions, px, size};

fn main() {
  Application::new().run(|cx: &mut App| {
    let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
    cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(680.), px(520.))),
        ..Default::default()
      },
      |_, cx| cx.new(|_| Cockpit::default()),
    )
    .expect("failed to open the Lyonbrown cockpit window");
    cx.activate(true);
  });
}

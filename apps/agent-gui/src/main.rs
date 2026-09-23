use agent_gui::composition::CompositionRoot;
use gpui::{App, AppContext, Application, Bounds, WindowBounds, WindowOptions, px, size};

fn main() {
  let composition = CompositionRoot::new().expect("invalid GUI dependency graph");
  let cockpit = composition
    .create_cockpit()
    .expect("failed to compose the Lyonbrown cockpit");

  Application::new().run(move |cx: &mut App| {
    let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
    cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(680.), px(520.))),
        ..Default::default()
      },
      |_, cx| cx.new(|_| cockpit),
    )
    .expect("failed to open the Lyonbrown cockpit window");
    cx.activate(true);
  });
}

use agent_gui::composition::CompositionRoot;
use agent_gui::startup::{GuiArgs, connect_native_client, sanitize_connection_error};
use clap::Parser;
use gpui::{App, AppContext, Application, Bounds, WindowBounds, WindowOptions, px, size};

#[tokio::main]
async fn main() {
  let args = GuiArgs::parse();
  let composition = match connect_native_client(&args).await {
    Ok(client) => CompositionRoot::with_native_client(client),
    Err(error) => CompositionRoot::unavailable(sanitize_connection_error(&error.to_string())),
  }
  .expect("invalid GUI dependency graph");
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
      |_, cx| {
        cx.new(|entity_cx| {
          let mut cockpit = cockpit;
          cockpit.start_remote_state_watch(entity_cx);
          cockpit
        })
      },
    )
    .expect("failed to open the Lyonbrown cockpit window");
    cx.activate(true);
  });
}

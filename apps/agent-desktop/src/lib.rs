#[cfg(test)]
extern crate self as agent_desktop;

mod chrome;
mod completion;
mod composition;
mod computer;
mod connection;
mod message;
mod operations;
mod panes;
mod presentation;
mod projection;
mod settings;
mod shell;
mod tabs;
mod workbench;

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;

#[cfg(test)]
#[path = "../tests/internal/completion_delivery.rs"]
mod completion_delivery;

#[cfg(test)]
#[path = "../tests/internal/frame_generation.rs"]
mod frame_generation;

#[cfg(test)]
#[path = "../tests/internal/computer_generation.rs"]
mod computer_generation;

pub use composition::{
  BrowserPort, ClientPort, ComputerPort, DesktopConfiguration, DesktopModule, ExecutorPort,
};
use gpui_kit::*;
pub use presentation::{ComputerSnapshot, DesktopSnapshot, WorkbenchSnapshot};
pub use shell::Desktop;
pub use tabs::ChatTabSnapshot;

pub fn init(cx: &mut App) {
  gpui_kit::init(cx);
  cx.bind_keys([
    KeyBinding::new("cmd-s", workbench::files::SaveFile, Some("WorkbenchEditor")),
    KeyBinding::new(
      "ctrl-s",
      workbench::files::SaveFile,
      Some("WorkbenchEditor"),
    ),
    KeyBinding::new("cmd-enter", shell::SendPrompt, Some("Composer > Input")),
    KeyBinding::new("ctrl-enter", shell::SendPrompt, Some("Composer > Input")),
    KeyBinding::new("cmd-l", shell::FocusComposer, Some("Desktop")),
    KeyBinding::new("ctrl-l", shell::FocusComposer, Some("Desktop")),
    KeyBinding::new("cmd-b", shell::ToggleSidebar, Some("Desktop")),
    KeyBinding::new("ctrl-b", shell::ToggleSidebar, Some("Desktop")),
    KeyBinding::new("cmd-alt-b", shell::ToggleWorkbench, Some("Desktop")),
    KeyBinding::new("ctrl-alt-b", shell::ToggleWorkbench, Some("Desktop")),
    KeyBinding::new("cmd-,", shell::OpenSettings, Some("Desktop")),
    KeyBinding::new("ctrl-,", shell::OpenSettings, Some("Desktop")),
    KeyBinding::new("cmd-t", shell::NewChatTab, Some("Desktop")),
    KeyBinding::new("ctrl-t", shell::NewChatTab, Some("Desktop")),
    KeyBinding::new("cmd-w", shell::CloseChatTab, Some("Desktop")),
    KeyBinding::new("ctrl-w", shell::CloseChatTab, Some("Desktop")),
    KeyBinding::new("escape", shell::CloseSettings, Some("DesktopSettings")),
    KeyBinding::new("cmd-q", shell::Quit, None),
    KeyBinding::new("alt-f4", shell::Quit, None),
  ]);
}

pub fn run() -> anyhow::Result<std::process::ExitCode> {
  if let Some(status) = agent_browser_runtime::dispatch_entry()? {
    return Ok(status);
  }
  let data_dir = std::env::var_os("LYONBROWN_DATA_DIR")
    .map(std::path::PathBuf::from)
    .map_or_else(agent_config::default_data_dir, Ok)?;
  let config_path = std::env::var_os("LYONBROWN_CONFIG").map(std::path::PathBuf::from);
  let settings = agent_config::load(&data_dir, config_path.as_deref())?;
  let _logs = agent_telemetry::init_with_options(
    &agent_telemetry::LogOptions {
      directory: settings.log_directory.clone(),
      retained_files: settings.log_retention,
      stderr: false,
    },
    "agent-desktop",
    &std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
  )?;
  tracing::info!(event = "desktop_started", "Native desktop starting");
  let module =
    composition::DesktopModule::new(fluxdi::Shared::new(composition::DesktopConfiguration {
      settings: settings.clone(),
      data_dir: data_dir.clone(),
      browser_executable: None,
    }));
  let runtime = module.executor()?.0.clone();
  let client = module.client()?;
  let browser = module.browser()?;
  let computer = module.computer()?;
  let configuration = module.configuration()?;
  gpui_kit::application()
    .with_assets(gpui_kit::assets::Assets)
    .run(move |cx| {
      init(cx);
      cx.on_action(|_: &shell::Quit, cx| cx.quit());
      cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
          cx.quit();
        }
      })
      .detach();
      let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
          None,
          size(px(1440.), px(920.)),
          cx,
        ))),
        window_min_size: Some(size(px(960.), px(640.))),
        ..gpui_kit::component::TitleBar::window_options()
      };
      if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
        window.activate_window();
        window.set_window_title("Lyonbrown");
        cx.new(|cx| {
          let mut desktop = Desktop::new(runtime.clone(), client.clone(), browser.clone(), computer.clone(), configuration.clone(), window, cx);
          desktop.connect(cx);
          desktop
        })
      }) {
        tracing::error!(event = "desktop_window_failed", error = %error, recovery = "Check the native graphics environment", "Unable to open native window");
        cx.quit();
      }
      cx.activate(true);
    });
  Ok(std::process::ExitCode::SUCCESS)
}

agent_browser_runtime::windows_bootstrap_entry!(run);

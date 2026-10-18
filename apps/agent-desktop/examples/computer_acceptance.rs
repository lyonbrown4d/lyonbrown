//! Visible production UI with an isolated daemon and a silent local model mock.
//! Discovery uses the real native factory. No permissions are requested and no
//! OS input occurs unless the user explicitly attaches and approves an action.
use agent_desktop::{Desktop, DesktopConfiguration, DesktopModule};
use agent_test_support::{MockModelServer, StandardProtocol};
use clap::Parser as _;
use gpui_kit::*;
use std::{collections::BTreeMap, sync::Arc};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;

fn main() -> anyhow::Result<()> {
  let acceptance = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/acceptance");
  std::fs::create_dir_all(&acceptance)?;
  let directory = tempfile::Builder::new()
    .prefix("computer-")
    .tempdir_in(acceptance)?;
  let workspace = directory.path().join("workspace");
  std::fs::create_dir(&workspace)?;
  std::env::set_current_dir(&workspace)?;
  let runtime = Arc::new(
    tokio::runtime::Builder::new_multi_thread()
      .worker_threads(3)
      .enable_all()
      .build()?,
  );
  let stop = CancellationToken::new();
  let stopped = stop.clone();
  let (model, daemon, endpoint) = runtime.block_on(async {
    let model = MockModelServer::start(Vec::new()).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let mut config = agentd::config::Config::try_parse_from([
      "agentd",
      "--listen",
      &listener.local_addr()?.to_string(),
      "--data-dir",
      directory
        .path()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("UTF-8 fixture path"))?,
      "--provider",
      StandardProtocol::ChatCompletions.provider(),
      "--model",
      "computer-acceptance-fixture",
      "--api-base",
      model.base_url(),
    ])?;
    config.provider_api_key = Some("synthetic-computer-fixture-key".into());
    config.computer_tools = true;
    let daemon = agentd::Daemon::open(config).await?;
    let client = agentd::client_service(daemon.clone());
    let workspace = agentd::workspace_service(daemon.clone());
    let browser = agentd::browser_service(daemon.clone());
    let computer = agentd::computer_runtime_service(daemon.clone());
    tokio::spawn(async move {
      if let Err(error) = tonic::transport::Server::builder()
        .add_service(client)
        .add_service(workspace)
        .add_service(browser)
        .add_service(computer)
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), stopped.cancelled_owned())
        .await
      {
        tracing::error!(error = %error, "Computer acceptance daemon stopped unexpectedly");
      }
    });
    anyhow::Ok((model, daemon, endpoint))
  })?;
  let settings = agent_config::load_with_environment(
    directory.path(),
    None,
    &BTreeMap::from([
      ("LYONBROWN_ENDPOINT".into(), endpoint),
      ("LYONBROWN_TOKEN".into(), daemon.auth_token().into()),
    ]),
  )?;
  let _logs = agent_telemetry::init(directory.path(), "computer-acceptance", "info")?;
  let module = DesktopModule::new(fluxdi::Shared::new(DesktopConfiguration {
    settings,
    data_dir: directory.path().into(),
    browser_executable: None,
  }));
  let configuration = module.configuration()?;
  let connector = module.client()?;
  let browser = module.browser()?;
  let computer = module.computer()?;
  println!("Computer UI acceptance workspace: {}", workspace.display());
  println!("Connect through Settings; select this fixture workspace for new chats.");
  gpui_kit::application()
    .with_assets(gpui_kit::assets::Assets)
    .run({
      let runtime = runtime.clone();
      move |cx| {
        agent_desktop::init(cx);
        cx.on_window_closed(|cx, _| {
          if cx.windows().is_empty() {
            cx.quit();
          }
        })
        .detach();
        if let Err(error) = gpui_kit::open_window(
          WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
              None,
              size(px(1440.), px(920.)),
              cx,
            ))),
            ..gpui_kit::component::TitleBar::window_options()
          },
          cx,
          move |window, cx| {
            window.set_window_title("Lyonbrown · Computer · isolated native acceptance");
            window.activate_window();
            cx.new(|cx| {
              Desktop::new(
                runtime,
                connector,
                browser,
                computer,
                configuration,
                window,
                cx,
              )
            })
          },
        ) {
          tracing::error!(error = %error, "Computer acceptance window failed");
          cx.quit();
        }
        cx.activate(true);
      }
    });
  stop.cancel();
  runtime.block_on(daemon.shutdown());
  anyhow::ensure!(
    runtime.block_on(model.requests()).is_empty(),
    "acceptance model was unexpectedly called"
  );
  Ok(())
}

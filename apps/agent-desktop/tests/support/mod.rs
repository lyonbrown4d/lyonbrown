// Shared native fixtures expose helpers used by different integration test binaries.
#![allow(dead_code)]
mod workspace;
use agent_desktop::{Desktop, DesktopConfiguration, DesktopModule, DesktopSnapshot};
use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol};
use clap::Parser as _;
use gpui_kit::{base::Root, test::TestWindowExt as _, *};
use std::{
  collections::BTreeMap,
  sync::Arc,
  time::{Duration, Instant},
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;

pub struct Harness {
  pub runtime: Arc<tokio::runtime::Runtime>,
  pub model: MockModelServer,
  pub daemon: agentd::Daemon,
  pub client: agent_client::AgentClient,
  pub directory: tempfile::TempDir,
  pub window: WindowHandle<Root>,
  pub view: Entity<Desktop>,
  shutdown: CancellationToken,
}

impl Harness {
  pub fn new(cx: &mut TestAppContext, replies: Vec<ScriptedReply>) -> Self {
    Self::with_optional_write_gate(cx, replies, None)
  }

  pub fn with_write_gate(
    cx: &mut TestAppContext,
    replies: Vec<ScriptedReply>,
    gate: Arc<tokio::sync::Semaphore>,
  ) -> Self {
    Self::with_optional_write_gate(cx, replies, Some(gate))
  }

  fn with_optional_write_gate(
    cx: &mut TestAppContext,
    replies: Vec<ScriptedReply>,
    write_gate: Option<Arc<tokio::sync::Semaphore>>,
  ) -> Self {
    let runtime = Arc::new(
      tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("fixture executor"),
    );
    let directory = tempfile::tempdir().expect("private desktop fixture");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("fixture workspace");
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let (model, daemon, client, endpoint) = runtime.block_on(async {
      let model = MockModelServer::start(replies)
        .await
        .expect("local synthetic model");
      let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("daemon listener");
      let endpoint = format!("http://{}", listener.local_addr().expect("daemon address"));
      let mut config = agentd::config::Config::try_parse_from([
        "agentd",
        "--listen",
        &listener.local_addr().expect("daemon address").to_string(),
        "--data-dir",
        directory.path().to_str().expect("test path"),
        "--provider",
        StandardProtocol::ChatCompletions.provider(),
        "--model",
        "fixture-model",
        "--api-base",
        model.base_url(),
        "--run-timeout-secs",
        "15",
      ])
      .expect("real daemon configuration");
      config.provider_api_key = Some("synthetic-model-key".into());
      let daemon = agentd::Daemon::open(config)
        .await
        .expect("real authenticated daemon");
      let service = agentd::client_service(daemon.clone());
      let expected_authorization = format!("Bearer {}", daemon.auth_token());
      let workspace_service =
        agent_protocol::v1::workspace_service_server::WorkspaceServiceServer::with_interceptor(
          workspace::WorkspaceProxy {
            daemon: daemon.clone(),
            write_gate,
          },
          move |request: tonic::Request<()>| {
            if request
              .metadata()
              .get("authorization")
              .is_some_and(|value| value.as_bytes() == expected_authorization.as_bytes())
            {
              Ok(request)
            } else {
              Err(tonic::Status::unauthenticated(
                "fixture credentials required",
              ))
            }
          },
        );
      let browser_service = agentd::browser_service(daemon.clone());
      let runtime_service = agentd::runtime_service(daemon.clone());
      tokio::spawn(async move {
        tonic::transport::Server::builder()
          .add_service(service)
          .add_service(workspace_service)
          .add_service(browser_service)
          .add_service(runtime_service)
          .serve_with_incoming_shutdown(TcpListenerStream::new(listener), stop.cancelled_owned())
          .await
          .expect("daemon RPC server");
      });
      let client = agent_client::AgentClient::connect(&endpoint, daemon.auth_token())
        .await
        .expect("authenticated fixture client");
      (model, daemon, client, endpoint)
    });
    let settings = agent_config::load_with_environment(
      directory.path(),
      None,
      &BTreeMap::from([
        ("LYONBROWN_ENDPOINT".into(), endpoint),
        ("LYONBROWN_TOKEN".into(), daemon.auth_token().into()),
      ]),
    )
    .expect("isolated shared configuration");
    let module = DesktopModule::new(fluxdi::Shared::new(DesktopConfiguration {
      settings: settings.clone(),
      data_dir: directory.path().into(),
      browser_executable: std::env::var_os("LYONBROWN_NATIVE_CLIENT").map(std::path::PathBuf::from),
    }));
    let connector = module.client().expect("production client port");
    let browser = module.browser().expect("production browser port");
    let (window, view) = cx.update(|cx| {
      agent_desktop::init(cx);
      let (window, view) = gpui_kit::open_window(
        WindowOptions {
          window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(1440.), px(920.)),
          })),
          ..gpui_kit::component::TitleBar::window_options()
        },
        cx,
        |window, cx| {
          cx.new(|cx| {
            Desktop::new(
              runtime.clone(),
              connector,
              browser,
              settings,
              directory.path().into(),
              window,
              cx,
            )
          })
        },
      )
      .expect("real desktop view");
      (window.downcast::<Root>().expect("native Root"), view)
    });
    cx.update_window(window.into(), |_, window, cx| {
      window.click("open-settings", cx);
      window.click("workspace", cx);
      window.press(&shortcut("a"), cx);
      window.input(workspace.to_str().expect("workspace path"), cx);
      window.click("close-settings", cx);
    })
    .expect("workspace input");
    Self {
      runtime,
      model,
      daemon,
      client,
      directory,
      window,
      view,
      shutdown,
    }
  }

  pub fn click(&self, id: impl Into<ElementId>, cx: &mut TestAppContext) {
    cx.update_window(self.window.into(), |_, window, cx| window.click(id, cx))
      .expect("native click");
  }

  pub fn connect(&self, cx: &mut TestAppContext) {
    self.click("open-settings", cx);
    self.click("connect", cx);
    self.click("close-settings", cx);
  }

  pub fn show_workbench(&self, cx: &mut TestAppContext) {
    if !self.snapshot(cx).workbench_expanded {
      self.click("toggle-workbench", cx);
    }
  }

  pub fn snapshot(&self, cx: &mut TestAppContext) -> DesktopSnapshot {
    cx.update(|cx| self.view.read(cx).snapshot(cx))
  }

  pub async fn wait(
    &self,
    cx: &mut TestAppContext,
    mut ready: impl FnMut(&DesktopSnapshot) -> bool,
  ) -> DesktopSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      cx.update_window(self.window.into(), |_, window, cx| window.render_frame(cx))
        .expect("render native frame");
      let snapshot = self.snapshot(cx);
      if ready(&snapshot) {
        return snapshot;
      }
      assert!(
        Instant::now() < deadline,
        "desktop condition timed out: {snapshot:?}"
      );
      // Tokio's real loopback sockets run on OS workers; also advance GPUI's test executor.
      std::thread::sleep(Duration::from_millis(10));
      cx.executor().timer(Duration::from_millis(100)).await;
    }
  }

  pub fn input_prompt(&self, cx: &mut TestAppContext, prompt: &str) {
    cx.update_window(self.window.into(), |_, window, cx| {
      window.press(&shortcut("l"), cx);
      window.input(prompt, cx);
      assert_eq!(self.view.read(cx).snapshot(cx).draft, prompt);
      window.press(&shortcut("enter"), cx);
    })
    .expect("native composer keyboard submission");
  }
}

impl Drop for Harness {
  fn drop(&mut self) {
    self.runtime.block_on(self.daemon.shutdown());
    self.shutdown.cancel();
  }
}

pub fn shortcut(key: &str) -> String {
  format!(
    "{}-{key}",
    if cfg!(target_os = "macos") {
      "cmd"
    } else {
      "ctrl"
    }
  )
}

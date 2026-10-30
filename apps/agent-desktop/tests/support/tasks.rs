use crate::remote_support::Remote;
use gpui_kit::{AppContext as _, TestAppContext, base::Root, test::TestWindowExt as _};
use std::{
  collections::BTreeMap,
  sync::Arc,
  time::{Duration, Instant},
};

pub struct TaskUi {
  pub runtime: Arc<tokio::runtime::Runtime>,
  pub _directory: tempfile::TempDir,
  pub window: gpui_kit::WindowHandle<Root>,
  pub view: gpui_kit::Entity<agent_desktop::Desktop>,
}

impl TaskUi {
  pub fn new(cx: &mut TestAppContext, endpoint: &str, token: &str) -> Self {
    let runtime = Arc::new(
      tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("desktop client executor"),
    );
    let directory = tempfile::tempdir().expect("desktop client data directory");
    let settings = agent_config::load_with_environment(
      directory.path(),
      None,
      &BTreeMap::from([
        ("LYONBROWN_ENDPOINT".into(), endpoint.to_string()),
        ("LYONBROWN_TOKEN".into(), token.to_string()),
      ]),
    )
    .expect("remote desktop settings");
    let module =
      agent_desktop::DesktopModule::new(fluxdi::Shared::new(agent_desktop::DesktopConfiguration {
        settings: settings.clone(),
        data_dir: directory.path().into(),
        browser_executable: None,
      }));
    let configuration = module.configuration().expect("desktop configuration");
    let connector = module.client().expect("desktop client connector");
    let browser = module.browser().expect("browser adapter");
    let computer = module.computer().expect("computer adapter");
    let (window, view) = cx.update(|cx| {
      agent_desktop::init(cx);
      let (window, view) = gpui_kit::open_window(
        gpui_kit::WindowOptions {
          window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
            origin: gpui_kit::Point::default(),
            size: gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(920.)),
          })),
          ..gpui_kit::component::TitleBar::window_options()
        },
        cx,
        |window, cx| {
          cx.new(|cx| {
            agent_desktop::Desktop::new(
              runtime.clone(),
              connector,
              browser,
              computer,
              configuration,
              window,
              cx,
            )
          })
        },
      )
      .expect("task desktop window");
      (window.downcast::<Root>().expect("native root"), view)
    });
    Self {
      runtime,
      _directory: directory,
      window,
      view,
    }
  }

  pub fn click(&self, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) {
    let id = id.into();
    cx.update_window(self.window.into(), |_, window, cx| {
      for _ in 0..40 {
        window.render_frame(cx);
        let Some(target) = window.try_find(id.clone()) else {
          break;
        };
        let panel = window.try_find("task-list").map(|panel| panel.bounds());
        let center = target.bounds().center();
        if target.visible() && panel.is_none_or(|panel| panel.contains(&center)) {
          break;
        }
        let target_y = target.bounds().center().y;
        let panel_y = window.find("task-list").bounds().center().y;
        let delta = if target_y < panel_y { 250. } else { -250. };
        window.scroll(
          "task-list",
          gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(delta))),
          cx,
        );
      }
      let target = window.find(id.clone());
      assert_ne!(target.disabled(), Some(true), "control disabled: {id:?}");
      window.click(id.clone(), cx);
      if matches!(&id, gpui_kit::ElementId::Name(name) if name.starts_with("task-accept-") || name.starts_with("task-reconcile-") || name.starts_with("task-cancel-") || name.starts_with("task-reject-")) {
        assert!(self.view.read(cx).snapshot(cx).workbench.task_pending, "click did not start operation: {id:?}, bounds {:?}, viewport {:?}", target.bounds(), window.viewport_size());
      }
    })
    .expect("click task UI");
  }

  pub fn connect(&self, cx: &mut TestAppContext) {
    self.click(cx, "open-settings");
    self.click(cx, "connect");
    self.click(cx, "close-settings");
  }

  pub fn select_session_and_open_tasks(&self, cx: &mut TestAppContext) {
    self.select_session_and_open_tasks_at(cx, 0);
  }

  pub fn select_session_and_open_tasks_at(&self, cx: &mut TestAppContext, index: usize) {
    self.click(cx, ("session", index));
    self.click(cx, "toggle-workbench");
    cx.update_window(self.window.into(), |_, window, cx| {
      window.render_frame(cx);
      window.click("menu", cx);
      window.within("popup-menu").click(4usize, cx);
      window.render_frame(cx);
      assert_eq!(
        window
          .within("tab-bar")
          .within("tabs")
          .find(4usize)
          .selected(),
        Some(true),
        "Tasks must be the selected dock panel before dispatch setup"
      );
      assert!(window.try_find("task-worker-id").is_some());
    })
    .expect("open durable Tasks page");
  }

  pub fn set_target(&self, cx: &mut TestAppContext, node_id: &str, endpoint: &str) {
    cx.update_window(self.window.into(), |_, window, cx| {
      window.click("task-worker-id", cx);
      window.input(node_id, cx);
      window.click("task-worker-endpoint", cx);
      window.input(endpoint, cx);
      window.click("task-instructions", cx);
      window.input(
        "Read the supplied workspace and return a concise result.",
        cx,
      );
    })
    .expect("fill explicit paired target and instructions");
  }

  pub async fn pump(&self, cx: &mut TestAppContext) {
    cx.update_window(self.window.into(), |_, window, cx| window.render_frame(cx))
      .expect("render task UI");
    self.runtime.block_on(async {
      tokio::time::sleep(Duration::from_millis(10)).await;
    });
    cx.executor().timer(Duration::from_millis(30)).await;
  }

  pub async fn wait(&self, cx: &mut TestAppContext, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() || cx.update(|cx| self.view.read(cx).snapshot(cx).workbench.task_pending) {
      assert!(
        Instant::now() < deadline,
        "task UI operation did not reach expected durable state: {:?}",
        cx.update(|cx| self.view.read(cx).snapshot(cx))
      );
      self.pump(cx).await;
    }
  }

  pub async fn wait_element(&self, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) {
    let id = id.into();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      let ready = cx
        .update_window(self.window.into(), |_, window, _| {
          window.try_find(id.clone()).is_some()
        })
        .expect("inspect task UI");
      if ready && !cx.update(|cx| self.view.read(cx).snapshot(cx).workbench.task_pending) {
        return;
      }
      assert!(
        Instant::now() < deadline,
        "task control did not appear: {id:?}"
      );
      self.pump(cx).await;
    }
  }

  pub async fn wait_connected(&self, cx: &mut TestAppContext) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      let ready = cx.update(|cx| {
        let snapshot = self.view.read(cx).snapshot(cx);
        snapshot.status.starts_with("Connected")
          && snapshot.local_state_loaded
          && !snapshot.local_state_loading
      });
      if ready {
        return;
      }
      assert!(
        Instant::now() < deadline,
        "Desktop did not authenticate to the paired Master"
      );
      self.pump(cx).await;
    }
  }

  pub async fn wait_active_run(&self, cx: &mut TestAppContext) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      if cx.update(|cx| self.view.read(cx).snapshot(cx).active_run.is_some()) {
        return;
      }
      assert!(
        Instant::now() < deadline,
        "selected chat did not load its active parent run"
      );
      self.pump(cx).await;
    }
  }

  pub async fn wait_selected_session(&self, cx: &mut TestAppContext, session_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      if cx.update(|cx| {
        self.view.read(cx).snapshot(cx).selected_session.as_deref() == Some(session_id)
      }) {
        return;
      }
      assert!(
        Instant::now() < deadline,
        "Desktop did not activate the requested session tab"
      );
      self.pump(cx).await;
    }
  }
}

pub async fn task_ids(remote: &Remote) -> Vec<String> {
  remote
    .pair
    .master
    .store
    .tasks()
    .expect("master task records")
    .into_iter()
    .filter(|task| task.envelope.session_id == remote.pair.session.id)
    .map(|task| task.envelope.task_id)
    .collect()
}

pub fn worker_target(remote: &Remote) -> agent_protocol::v1::Node {
  remote.target()
}

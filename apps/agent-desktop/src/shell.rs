use crate::workbench;
use agent_client::AgentClient;
use gpui_kit::component::{
  ActiveTheme,
  dock::DockArea,
  input::{InputState, TextareaState},
};
use gpui_kit::{TestSupportExt as _, *};
use std::sync::Arc;

actions!(
  desktop,
  [
    SendPrompt,
    FocusComposer,
    Quit,
    ToggleSidebar,
    ToggleWorkbench,
    OpenSettings,
    CloseSettings,
    NavigateBack,
    NavigateForward,
    NewChatTab,
    CloseChatTab
  ]
);

pub struct Desktop {
  pub(super) connector: fluxdi::Shared<crate::composition::ClientPort>,
  pub(super) runtime: Arc<tokio::runtime::Runtime>,
  pub(super) data_dir: std::path::PathBuf,
  pub(super) sender: async_channel::Sender<crate::tabs::ScopedUpdate>,
  pub(super) client: Option<AgentClient>,
  pub(crate) vm: crate::view_model::DesktopViewModel,
  pub(super) endpoint: Entity<InputState>,
  pub(super) token: Entity<InputState>,
  pub(super) workspace: Entity<InputState>,
  pub(super) composer: Entity<TextareaState>,
  pub(super) recovery: Option<crate::recovery::RecoveryState>,
  pub(super) dock: Entity<DockArea>,
  pub(super) main_layout: Entity<gpui_kit::component::resizable::ResizableState>,
  pub(super) tasks: Entity<workbench::Workbench>,
  pub(super) show_sidebar: bool,
  pub(super) show_workbench: bool,
  pub(super) router: crate::router::Router,
  pub(super) settings_focus: FocusHandle,
  pub(super) resources: crate::resources::ResourcePanels,
  pub(super) navigation: crate::tabs::Navigation,
  pub(super) persistence_sender: Option<tokio::sync::mpsc::Sender<crate::persistence::Command>>,
  pub(super) persistence_scope: Option<String>,
  pub(super) persistence_generation: u64,
  pub(super) persistence_loading: bool,
  pub(super) persistence_loaded: bool,
  pub(super) last_persisted: Option<String>,
  pub(super) last_enqueued: Option<String>,
  pub(super) persistence_sequence: u64,
  pub(super) latest_state: Option<crate::local_state::DesktopState>,
  pub(super) latest_shared: crate::persistence::LatestSnapshot,
  pub(super) flush_control: crate::persistence::FlushControl,
  pub(super) watch_task: Option<tokio::task::JoinHandle<()>>,
  _completion: Task<()>,
  _pending_work: [Subscription; 2],
  _local_state_watch: [Subscription; 4],
  _persistence_quit: Subscription,
  _close_started: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Desktop {
  fn drop(&mut self) {
    if let (Some(sender), Some(scope)) = (&self.persistence_sender, &self.persistence_scope) {
      let sender = sender.clone();
      let (ack, completion) = tokio::sync::oneshot::channel();
      let command = crate::persistence::Command::Stop {
        scope: scope.clone(),
        generation: self.persistence_generation,
        sequence: self.persistence_sequence,
        state: self.latest_state.clone().map(Box::new),
        ack: Some(ack),
      };
      if let Ok(mut flush) = self.flush_control.lock() {
        flush.sender = None;
        flush.completion = Some(completion);
      }
      self.runtime.spawn(async move {
        let _ = sender.send(command).await;
      });
    }
    // Stop this window's subscription; daemon runs have an independent lifetime.
    if let Some(task) = self.watch_task.take() {
      task.abort();
    }
  }
}

impl Desktop {
  pub fn new(
    runtime: Arc<tokio::runtime::Runtime>,
    connector: fluxdi::Shared<crate::composition::ClientPort>,
    browser: fluxdi::Shared<crate::composition::BrowserPort>,
    computer: fluxdi::Shared<crate::composition::ComputerPort>,
    configuration: fluxdi::Shared<crate::DesktopConfiguration>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let settings = &configuration.settings;
    let data_dir = &configuration.data_dir;
    // Bound queued presentation events; async producers wait without blocking either executor.
    let (sender, receiver) = crate::completion::channel(256);
    let (dock, tasks) = workbench::build(runtime.clone(), browser, computer, window, cx);
    let resources = crate::resources::ResourcePanels::new(runtime.clone(), window, cx);
    let workspace = cx.new(|cx| {
      InputState::new(window, cx)
        .default_value(
          std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        )
        .placeholder("Workspace path on daemon")
    });
    let composer = cx.new(|cx| {
      TextareaState::new(window, cx)
        .submit_on_enter(true)
        .placeholder("描述你的任务，或补充要求…")
    });
    let main_layout = cx.new(|_| gpui_kit::component::resizable::ResizableState::default());
    let flush_control = std::sync::Arc::new(std::sync::Mutex::new(
      crate::persistence::FlushState::default(),
    ));
    let close_control = flush_control.clone();
    let close_runtime = runtime.clone();
    let close_view = cx.entity().downgrade();
    let close_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let close_started_in_callback = close_started.clone();
    window.on_window_should_close(cx, move |window, app| {
      if close_started_in_callback.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return false;
      }
      let _ = close_view.update(app, |desktop, cx| desktop.queue_snapshot(cx));
      let window = window.window_handle();
      let runtime = close_runtime.clone();
      let control = close_control.clone();
      app
        .spawn(async move |cx| {
          let _runtime = runtime;
          crate::persistence::quit_flush(control).await;
          let _ = cx.update_window(window, |_, window, _| window.remove_window());
        })
        .detach();
      false
    });
    let quit_control = flush_control.clone();
    let persistence_quit = cx.on_app_quit(move |desktop, cx| {
      desktop.queue_snapshot(cx);
      crate::persistence::quit_flush(quit_control.clone())
    });
    let local_state_watch = [
      cx.observe(&composer, |desktop, _, cx| desktop.queue_snapshot(cx)),
      cx.observe(&workspace, |desktop, _, cx| desktop.queue_snapshot(cx)),
      cx.observe(&main_layout, |desktop, _, cx| desktop.queue_snapshot(cx)),
      cx.observe(&dock, |desktop, _, cx| desktop.queue_snapshot(cx)),
    ];
    composer.read(cx).focus_handle(cx).focus(window, cx);
    let completion = cx.spawn_in(window, async move |view, cx| {
      let mut remaining = 32;
      while let Ok(update) = receiver.recv().await {
        if view
          .update_in(cx, |view, window, cx| view.apply_scoped(update, window, cx))
          .is_err()
        {
          break;
        }
        remaining -= 1;
        if remaining == 0 {
          remaining = 32;
          // Yield through GPUI's scheduler so a stream burst cannot monopolize input/rendering.
          cx.foreground_executor().spawn(async {}).await;
        }
      }
    });
    let computer = tasks.read(cx).computer.clone();
    let pending_work = [
      cx.observe(&tasks, |view, _, cx| {
        view.clear_finished_wait(cx);
        view.queue_snapshot(cx);
      }),
      cx.observe(&computer, |view, _, cx| view.clear_finished_wait(cx)),
    ];
    Self {
      runtime,
      data_dir: data_dir.clone(),
      connector,
      sender,
      client: None,
      vm: crate::view_model::DesktopViewModel::default(),
      endpoint: cx.new(|cx| {
        InputState::new(window, cx)
          .default_value(settings.endpoint.clone())
          .placeholder("Daemon address")
      }),
      token: cx.new(|cx| {
        InputState::new(window, cx)
          .masked(true)
          .default_value(
            settings
              .credential("daemon")
              .unwrap_or_else(|| crate::connection::local_token(data_dir)),
          )
          .placeholder("Access token")
      }),
      workspace,
      composer,
      recovery: None,
      dock,
      main_layout,
      tasks,
      show_sidebar: true,
      show_workbench: false,
      router: crate::router::Router::default(),
      settings_focus: cx.focus_handle(),
      resources,
      navigation: crate::tabs::Navigation::default(),
      persistence_sender: None,
      persistence_scope: None,
      persistence_generation: 0,
      persistence_loading: false,
      persistence_loaded: false,
      last_persisted: None,
      last_enqueued: None,
      persistence_sequence: 0,
      latest_state: None,
      latest_shared: std::sync::Arc::new(std::sync::Mutex::new(None)),
      flush_control,
      watch_task: None,
      _completion: completion,
      _pending_work: pending_work,
      _local_state_watch: local_state_watch,
      _persistence_quit: persistence_quit,
      _close_started: close_started,
    }
  }

  fn clear_finished_wait(&mut self, cx: &mut Context<Self>) {
    let tasks = self.tasks.read(cx);
    let finished = match self.vm.error.as_deref() {
      Some(crate::tabs::SAVE_WAIT_MESSAGE) => !tasks.pending_save,
      Some(crate::tabs::BROWSER_WAIT_MESSAGE) => !tasks.pending_browser_operation,
      Some(crate::tabs::COMPUTER_WAIT_MESSAGE) => !tasks.computer.read(cx).pending,
      _ => false,
    };
    if finished {
      self.vm.error = None;
      cx.notify();
    }
  }
}

impl Render for Desktop {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.queue_snapshot(cx);
    div()
      .id("desktop")
      .test_support()
      .key_context("Desktop")
      .on_action(cx.listener(|this, _: &SendPrompt, window, cx| this.send(window, cx)))
      .on_action(cx.listener(|this, _: &FocusComposer, window, cx| {
        this.close_settings(window, cx);
      }))
      .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
        if !this.persistence_loading && !this.navigation.connecting {
          this.show_sidebar = !this.show_sidebar;
        }
        cx.notify();
      }))
      .on_action(cx.listener(|this, _: &ToggleWorkbench, _, cx| {
        if !this.persistence_loading && !this.navigation.connecting {
          this.show_workbench = !this.show_workbench;
        }
        cx.notify();
      }))
      .on_action(cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)))
      .on_action(cx.listener(|this, _: &CloseSettings, window, cx| {
        this.close_settings(window, cx);
      }))
      .on_action(cx.listener(|this, _: &NavigateBack, window, cx| {
        if this.router.back() {
          this.focus_route(window, cx);
        }
      }))
      .on_action(cx.listener(|this, _: &NavigateForward, window, cx| {
        if this.router.forward() {
          this.focus_route(window, cx);
        }
      }))
      .on_action(cx.listener(|this, _: &NewChatTab, window, cx| this.new_chat_tab(window, cx)))
      .on_action(cx.listener(|this, _: &CloseChatTab, window, cx| {
        if this.router.is_settings() {
          this.close_settings(window, cx);
        } else {
          this.close_active_chat_tab(window, cx);
        }
      }))
      .size_full()
      .flex()
      .flex_col()
      .bg(cx.theme().background)
      .text_color(cx.theme().foreground)
      .text_sm()
      .child(self.chrome(window, cx))
      .child(self.route_layout(window, cx))
  }
}

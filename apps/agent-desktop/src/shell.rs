use crate::workbench;
use agent_client::AgentClient;
use agent_protocol::v1::{Approval, Run, Session};
use gpui_kit::component::{
  ActiveTheme,
  dock::DockArea,
  input::{InputState, TextareaState},
  resizable::{h_resizable, resizable_panel},
};
use gpui_kit::{TestSupportExt as _, prelude::FluentBuilder as _, *};
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
    NewChatTab,
    CloseChatTab
  ]
);

pub struct Desktop {
  pub(super) connector: fluxdi::Shared<crate::composition::ClientPort>,
  pub(super) runtime: Arc<tokio::runtime::Runtime>,
  pub(super) sender: async_channel::Sender<crate::tabs::ScopedUpdate>,
  pub(super) client: Option<AgentClient>,
  pub(super) sessions: Vec<Session>,
  pub(super) selected: Option<String>,
  pub(super) runs: Vec<Run>,
  pub(super) active_run: Option<String>,
  pub(super) cursor: u64,
  pub(super) busy: bool,
  pub(super) error: Option<String>,
  pub(super) status: String,
  pub(super) conversation: Vec<crate::message::ChatMessage>,
  pub(super) approvals: Vec<Approval>,
  pub(super) endpoint: Entity<InputState>,
  pub(super) token: Entity<InputState>,
  pub(super) workspace: Entity<InputState>,
  pub(super) composer: Entity<TextareaState>,
  dock: Entity<DockArea>,
  pub(super) tasks: Entity<workbench::Workbench>,
  pub(super) show_sidebar: bool,
  pub(super) show_workbench: bool,
  pub(super) show_settings: bool,
  pub(super) navigation: crate::tabs::Navigation,
  pub(super) watch_task: Option<tokio::task::JoinHandle<()>>,
  _completion: Task<()>,
  _pending_work: [Subscription; 2],
}

impl Drop for Desktop {
  fn drop(&mut self) {
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
    let composer = cx.new(|cx| {
      TextareaState::new(window, cx).placeholder("Ask Lyonbrown to work on your project…")
    });
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
      cx.observe(&tasks, |view, _, cx| view.clear_finished_wait(cx)),
      cx.observe(&computer, |view, _, cx| view.clear_finished_wait(cx)),
    ];
    Self {
      runtime,
      connector,
      sender,
      client: None,
      sessions: Vec::new(),
      selected: None,
      runs: Vec::new(),
      active_run: None,
      cursor: 0,
      busy: false,
      error: None,
      status: "Disconnected".into(),
      conversation: Vec::new(),
      approvals: Vec::new(),
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
      workspace: cx.new(|cx| {
        InputState::new(window, cx)
          .default_value(
            std::env::current_dir()
              .map(|path| path.display().to_string())
              .unwrap_or_default(),
          )
          .placeholder("Workspace path on daemon")
      }),
      composer,
      dock,
      tasks,
      show_sidebar: true,
      show_workbench: false,
      show_settings: false,
      navigation: crate::tabs::Navigation::default(),
      watch_task: None,
      _completion: completion,
      _pending_work: pending_work,
    }
  }

  fn clear_finished_wait(&mut self, cx: &mut Context<Self>) {
    let tasks = self.tasks.read(cx);
    let finished = match self.error.as_deref() {
      Some(crate::tabs::SAVE_WAIT_MESSAGE) => !tasks.pending_save,
      Some(crate::tabs::BROWSER_WAIT_MESSAGE) => !tasks.pending_browser_operation,
      Some(crate::tabs::COMPUTER_WAIT_MESSAGE) => !tasks.computer.read(cx).pending,
      _ => false,
    };
    if finished {
      self.error = None;
      cx.notify();
    }
  }
}

impl Render for Desktop {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    div()
      .id("desktop")
      .test_support()
      .key_context("Desktop")
      .on_action(cx.listener(|this, _: &SendPrompt, window, cx| this.send(window, cx)))
      .on_action(cx.listener(|this, _: &FocusComposer, window, cx| {
        this.show_settings = false;
        this.composer.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
      }))
      .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
        this.show_sidebar = !this.show_sidebar;
        cx.notify();
      }))
      .on_action(cx.listener(|this, _: &ToggleWorkbench, _, cx| {
        this.show_workbench = !this.show_workbench;
        cx.notify();
      }))
      .on_action(cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)))
      .on_action(cx.listener(|this, _: &CloseSettings, window, cx| {
        this.show_settings = false;
        this.composer.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
      }))
      .on_action(cx.listener(|this, _: &NewChatTab, window, cx| this.new_chat_tab(window, cx)))
      .on_action(cx.listener(|this, _: &CloseChatTab, window, cx| {
        if this.show_settings {
          this.show_settings = false;
          this.composer.read(cx).focus_handle(cx).focus(window, cx);
          cx.notify();
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
      .when(self.show_settings, |this| {
        this.child(self.settings_view(cx))
      })
      .when(!self.show_settings, |this| {
        this.child(self.chat_tabs(cx)).child(
          div().flex_1().min_h_0().child(
            h_resizable("desktop-layout")
              .when(self.show_sidebar, |this| {
                this.child(
                  resizable_panel()
                    .size(px(242.))
                    .size_range(px(190.)..px(340.))
                    .child(self.sidebar(cx)),
                )
              })
              .child(
                resizable_panel()
                  .size_range(px(360.)..px(2000.))
                  .child(self.conversation(cx)),
              )
              .when(self.show_workbench, |this| {
                this.child(
                  resizable_panel()
                    .size(px(460.))
                    .size_range(px(260.)..px(1600.))
                    .child(self.dock.clone()),
                )
              }),
          ),
        )
      })
  }
}

use agent_client::AgentClient;
use agent_protocol::v1::{
  BrowserFrame, BrowserSpace, BrowserTab, Run, WorkspaceEntry, WorkspaceFile,
};
use gpui_kit::component::{
  diff::DiffState,
  input::{EditorState, InputState},
};
use gpui_kit::*;
use std::{
  sync::{Arc, mpsc},
  time::{Duration, Instant},
};

pub(crate) enum Reply {
  Files(String, Vec<WorkspaceEntry>),
  BackgroundFiles(String, Vec<WorkspaceEntry>),
  File(WorkspaceFile),
  Saved(WorkspaceFile),
  SaveError(String),
  Patch(String),
  Tab(BrowserSpace, BrowserTab),
  TabOpenFailed(BrowserSpace, String),
  Control(BrowserSpace),
  Frame(u64, BrowserFrame, bool),
  NoFrame(u64),
  FrameError(u64, String),
  Input,
  Closed,
  InputError(String),
  BrowserOperationError(String),
  Attached(agent_browser_runtime::RuntimeAttachment),
  Changed(Vec<String>),
  Revision(String, String),
  WatchError(String),
  Error(String),
}

pub struct Workbench {
  pub(crate) computer: Entity<crate::computer::ComputerPane>,
  pub(crate) runtime: Arc<tokio::runtime::Runtime>,
  pub(crate) browser: fluxdi::Shared<crate::composition::BrowserPort>,
  pub(crate) attachment: Option<agent_browser_runtime::RuntimeAttachment>,
  pub(crate) sender: mpsc::Sender<(u64, Reply)>,
  pub(crate) generation: u64,
  pub(crate) client: Option<AgentClient>,
  pub(crate) session: Option<String>,
  pub(crate) runs: Vec<Run>,
  pub(crate) files: Vec<WorkspaceEntry>,
  pub(crate) directory: String,
  pub(crate) file: Option<WorkspaceFile>,
  pub(super) editor_drafts: std::collections::BTreeMap<String, super::drafts::EditorDraft>,
  pub(crate) editor: Entity<EditorState>,
  pub(crate) diff: Entity<DiffState>,
  pub(crate) patch: String,
  pub(crate) busy: bool,
  pub(crate) pending_save: bool,
  pub(crate) pending_browser_operation: bool,
  pub(crate) connection_pending: bool,
  pub(crate) error: Option<String>,
  pub(crate) browser_error: Option<String>,
  pub(crate) url: Entity<InputState>,
  pub(crate) space: Option<BrowserSpace>,
  pub(crate) tab: Option<BrowserTab>,
  pub(crate) browser_sessions:
    std::collections::BTreeMap<String, (BrowserSpace, Option<BrowserTab>)>,
  pub(crate) frame: Option<(BrowserFrame, Arc<RenderImage>)>,
  pub(crate) browser_focus: FocusHandle,
  pub(crate) marked: Option<String>,
  pub(crate) browser_bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
  pub(crate) frame_pending: bool,
  pub(crate) frame_generation: u64,
  pub(crate) requested_viewport: Option<(u32, u32)>,
  pub(crate) input_pending: bool,
  pub(crate) input_frame_pending: bool,
  pub(crate) input_queue: std::collections::VecDeque<agent_protocol::v1::BrowserInput>,
  pub(crate) inputs_sent: u64,
  pub(crate) last_pointer: Option<(i32, i32)>,
  pub(crate) watch: Option<tokio::task::JoinHandle<()>>,
  last_frame: Instant,
  _editor_subscription: Subscription,
  _poll: Task<()>,
}

impl Workbench {
  pub(crate) fn new(
    runtime: Arc<tokio::runtime::Runtime>,
    browser: fluxdi::Shared<crate::composition::BrowserPort>,
    computer: fluxdi::Shared<crate::composition::ComputerPort>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let (sender, receiver) = mpsc::channel();
    let editor = cx.new(|cx| {
      EditorState::new(window, cx)
        .line_number(true)
        .placeholder("Open a workspace file")
    });
    let editor_subscription = cx.observe(&editor, |_, _, cx| cx.notify());
    let poll = cx.spawn_in(window, async move |view, cx| {
      loop {
        cx.background_executor()
          .timer(Duration::from_millis(80))
          .await;
        if view
          .update_in(cx, |view, window, cx| {
            for (generation, reply) in receiver.try_iter() {
              if generation == view.generation {
                view.apply(reply, window, cx);
              }
            }
            if view.last_frame.elapsed() >= Duration::from_millis(160) {
              view.fetch_frame(cx);
            }
          })
          .is_err()
        {
          break;
        }
      }
    });
    let computer =
      cx.new(|cx| crate::computer::ComputerPane::new(runtime.clone(), computer, window, cx));
    Self {
      computer,
      runtime,
      browser,
      attachment: None,
      sender,
      generation: 0,
      client: None,
      session: None,
      runs: Vec::new(),
      files: Vec::new(),
      directory: String::new(),
      file: None,
      editor_drafts: Default::default(),
      editor,
      diff: cx.new(|cx| DiffState::new(Vec::new(), cx)),
      patch: String::new(),
      busy: false,
      pending_save: false,
      pending_browser_operation: false,
      connection_pending: false,
      error: None,
      browser_error: None,
      url: cx.new(|cx| {
        InputState::new(window, cx)
          .placeholder("https://…")
          .default_value("about:blank")
      }),
      space: None,
      tab: None,
      browser_sessions: std::collections::BTreeMap::new(),
      frame: None,
      browser_focus: cx.focus_handle(),
      marked: None,
      browser_bounds: std::rc::Rc::new(std::cell::Cell::new(Bounds::default())),
      frame_pending: false,
      frame_generation: 0,
      requested_viewport: None,
      input_pending: false,
      input_frame_pending: false,
      input_queue: std::collections::VecDeque::new(),
      inputs_sent: 0,
      last_pointer: None,
      watch: None,
      last_frame: Instant::now(),
      _editor_subscription: editor_subscription,
      _poll: poll,
    }
  }

  pub fn bind(
    &mut self,
    client: Option<AgentClient>,
    session: Option<String>,
    runs: Vec<Run>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    self.computer.update(cx, |computer, cx| {
      computer.bind(client.clone(), session.clone(), window, cx)
    });
    self.runs = runs;
    let connected_changed = self.client.is_some() != client.is_some();
    self.client = client.clone();
    if self.session != session || connected_changed {
      if connected_changed {
        self.editor_drafts.clear();
      } else {
        self.stash_editor(cx);
      }
      self.generation += 1;
      if let (Some(id), Some(space)) = (&self.session, &self.space) {
        self
          .browser_sessions
          .insert(id.clone(), (space.clone(), self.tab.clone()));
      }
      self.session = session;
      self.client = client;
      self.files.clear();
      self.directory.clear();
      self.file = None;
      self.error = None;
      self.browser_error = None;
      self.marked = None;
      let restored = self
        .session
        .as_ref()
        .and_then(|id| self.browser_sessions.get(id))
        .cloned();
      self.space = restored.as_ref().map(|(space, _)| space.clone());
      self.tab = restored.and_then(|(_, tab)| tab);
      self.invalidate_frames();
      self.requested_viewport = None;
      self.input_pending = false;
      self.input_frame_pending = false;
      self.input_queue.clear();
      self.inputs_sent = 0;
      self.last_pointer = None;
      self.busy = false;
      self.patch.clear();
      self.restore_editor(window, cx);
      self
        .diff
        .update(cx, |diff, cx| diff.set_files(Vec::new(), cx));
      self.watch_workspace();
      if self.session.is_some() {
        self.load_directory(String::new(), cx);
      }
    }
    cx.notify();
  }

  pub(crate) fn request<F>(&self, future: F)
  where
    F: std::future::Future<Output = anyhow::Result<Reply>> + Send + 'static,
  {
    let sender = self.sender.clone();
    let generation = self.generation;
    self.runtime.spawn(async move {
      let reply = future
        .await
        .unwrap_or_else(|error| Reply::Error(error.to_string()));
      let _ = sender.send((generation, reply));
    });
  }

  pub(crate) fn fetch_frame(&mut self, cx: &mut Context<Self>) {
    self.last_frame = Instant::now();
    self.resize_browser(cx);
    if let Some(error) = self
      .attachment
      .as_ref()
      .and_then(|attachment| attachment.error())
    {
      self.browser_error = Some(format!(
        "Browser runtime disconnected: {error}. Restart runtime to open a new page; prior actions will not be replayed."
      ));
      return;
    }
    if self.frame_pending || self.browser_error.is_some() {
      return;
    }
    let (Some(client), Some(tab)) = (self.client.clone(), self.tab.clone()) else {
      return;
    };
    self.frame_pending = true;
    let generation = self.frame_generation;
    let refresh_input = self.input_frame_pending;
    let sequence = if refresh_input {
      0
    } else {
      self.frame.as_ref().map_or(0, |(frame, _)| frame.sequence)
    };
    self.request(async move {
      Ok(match client.browser_frame(&tab.id, sequence).await {
        Ok(frame) => Reply::Frame(generation, frame, refresh_input),
        Err(error) if crate::connection::frame_not_ready(&error) => Reply::NoFrame(generation),
        Err(error) => Reply::FrameError(generation, error.to_string()),
      })
    });
  }

  pub(crate) fn invalidate_frames(&mut self) {
    self.frame_generation = self.frame_generation.saturating_add(1);
    self.frame = None;
    self.frame_pending = false;
  }
}

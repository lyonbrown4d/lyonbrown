use agent_client::AgentClient;
use agent_protocol::v1::{
  BrowserFrame, BrowserSpace, BrowserTab, Run, Task as AgentTask, WorkspaceEntry, WorkspaceFile,
};
use gpui_kit::component::{
  diff::DiffState,
  input::{EditorState, InputState, TextareaState},
  select::SelectState,
};
use gpui_kit::*;
use std::{
  sync::Arc,
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
  Tasks(u64, Vec<AgentTask>, String),
  Task(u64, AgentTask),
  TaskError(u64, String, Option<bool>),
}

#[derive(Clone)]
pub(crate) struct DispatchIntent {
  pub task_id: String,
  pub attempt_id: String,
  pub idempotency_key: String,
  pub uncertain: bool,
}

#[derive(Clone, Default)]
pub(crate) struct TaskDispatchContext {
  pub intent: Option<DispatchIntent>,
  pub history: Vec<DispatchIntent>,
}

pub struct Workbench {
  pub(crate) computer: Entity<crate::computer::ComputerPane>,
  pub(crate) runtime: Arc<tokio::runtime::Runtime>,
  pub(crate) browser: fluxdi::Shared<crate::composition::BrowserPort>,
  pub(crate) attachment: Option<agent_browser_runtime::RuntimeAttachment>,
  pub(crate) sender: async_channel::Sender<(u64, Reply)>,
  pub(crate) generation: u64,
  pub(crate) client: Option<AgentClient>,
  pub(crate) session: Option<String>,
  pub(crate) runs: Vec<Run>,
  pub(crate) tasks: Vec<AgentTask>,
  pub(crate) task_cursor: String,
  pub(crate) task_pending: bool,
  pub(crate) recovery_tasks_only: bool,
  pub(crate) task_generation: u64,
  pub(crate) dispatch_intent: Option<DispatchIntent>,
  pub(crate) dispatch_history: Vec<DispatchIntent>,
  pub(crate) dispatch_contexts: std::collections::BTreeMap<String, TaskDispatchContext>,
  pub(crate) task_target_id: Entity<InputState>,
  pub(crate) task_target_endpoint: Entity<InputState>,
  pub(crate) task_instructions: Entity<TextareaState>,
  pub(crate) task_capabilities: Entity<SelectState<Vec<String>>>,
  pub(crate) files: Vec<WorkspaceEntry>,
  pub(crate) directory: String,
  pub(crate) file: Option<WorkspaceFile>,
  pub(super) editor_drafts: std::collections::BTreeMap<String, super::drafts::EditorDraft>,
  pub(super) unbound_editor_drafts: Vec<super::drafts::EditorDraft>,
  pub(super) unbound_editor: Option<super::drafts::EditorDraft>,
  pub(super) restored_editor_draft: bool,
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
  pub(super) last_frame: Instant,
  _editor_subscription: Subscription,
  _completion: Task<()>,
  _frame_refresh: Task<()>,
}

impl Workbench {
  pub(crate) fn new(
    runtime: Arc<tokio::runtime::Runtime>,
    browser: fluxdi::Shared<crate::composition::BrowserPort>,
    computer: fluxdi::Shared<crate::composition::ComputerPort>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    // File/watch completions backpressure Tokio rather than accumulating without a limit.
    let (sender, receiver) = crate::completion::channel(64);
    let editor = cx.new(|cx| {
      EditorState::new(window, cx)
        .line_number(true)
        .placeholder("Open a workspace file")
    });
    let editor_subscription = cx.observe(&editor, |_, _, cx| cx.notify());
    let task_target_id =
      cx.new(|cx| InputState::new(window, cx).placeholder("Paired Worker node ID"));
    let task_target_endpoint =
      cx.new(|cx| InputState::new(window, cx).placeholder("Worker endpoint (HTTPS)"));
    let task_instructions = cx.new(|cx| {
      TextareaState::new(window, cx)
        .placeholder("Task instructions for Worker")
        .submit_on_enter(false)
    });
    let task_capabilities = cx.new(|cx| {
      SelectState::new(
        vec![
          "Read only".to_string(),
          "Read and write after approval".to_string(),
        ],
        Some(gpui_kit::base::IndexPath::new(0)),
        window,
        cx,
      )
    });
    let completion = cx.spawn_in(window, async move |view, cx| {
      let mut remaining = 32;
      while let Ok((generation, reply)) = receiver.recv().await {
        if view
          .update_in(cx, |view, window, cx| {
            if generation == view.generation {
              view.apply(reply, window, cx);
            }
          })
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
    // Frame production has its own cadence, independent of completion delivery.
    let frame_refresh = cx.spawn_in(window, async move |view, cx| {
      loop {
        cx.background_executor()
          .timer(Duration::from_millis(160))
          .await;
        if view
          .update_in(cx, |view, _, cx| {
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
      tasks: Vec::new(),
      task_cursor: String::new(),
      task_pending: false,
      recovery_tasks_only: false,
      task_generation: 0,
      dispatch_intent: None,
      dispatch_history: Vec::new(),
      dispatch_contexts: Default::default(),
      task_target_id,
      task_target_endpoint,
      task_instructions,
      task_capabilities,
      files: Vec::new(),
      directory: String::new(),
      file: None,
      editor_drafts: Default::default(),
      unbound_editor_drafts: Vec::new(),
      unbound_editor: None,
      restored_editor_draft: false,
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
      _completion: completion,
      _frame_refresh: frame_refresh,
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
      let previous_session = self.session.clone();
      if connected_changed {
        self.dispatch_contexts.clear();
      } else if let Some(previous_session) = previous_session {
        self.dispatch_contexts.insert(
          previous_session,
          TaskDispatchContext {
            intent: self.dispatch_intent.clone(),
            history: self.dispatch_history.clone(),
          },
        );
      }
      self.task_generation = self.task_generation.saturating_add(1);
      self.task_pending = false;
      self.recovery_tasks_only = false;
      self.tasks.clear();
      self.task_cursor.clear();
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
      let context = self
        .session
        .as_ref()
        .and_then(|id| self.dispatch_contexts.get(id))
        .cloned()
        .unwrap_or_default();
      self.dispatch_intent = context.intent;
      self.dispatch_history = context.history;
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
      let _ = sender.send((generation, reply)).await;
    });
  }
}

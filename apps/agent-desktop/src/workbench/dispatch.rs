use agent_protocol::v1::{
  BrowserFrame, BrowserSpace, BrowserTab, Task as AgentTask, WorkspaceEntry, WorkspaceFile,
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

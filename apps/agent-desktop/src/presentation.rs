use crate::Desktop;
use gpui_kit::*;

/// Read-only presentation state for native embedding, diagnostics and UI integration checks.
#[derive(Clone, Debug)]
pub struct DesktopSnapshot {
  pub sidebar_expanded: bool,
  pub workbench_expanded: bool,
  pub settings_open: bool,
  pub local_state_loading: bool,
  pub local_state_loaded: bool,
  pub layout_sizes: Vec<f32>,
  pub tabs: Vec<crate::tabs::ChatTabSnapshot>,
  pub status: String,
  pub session_titles: Vec<String>,
  pub selected_session: Option<String>,
  pub active_run: Option<String>,
  pub event_cursor: u64,
  pub draft: String,
  pub messages: Vec<(String, String)>,
  pub approvals: Vec<(String, String, String)>,
  pub error: Option<String>,
  pub busy: bool,
  pub workbench: WorkbenchSnapshot,
}

#[derive(Clone, Debug)]
pub struct WorkbenchSnapshot {
  pub task_pending: bool,
  pub computer: ComputerSnapshot,
  pub paths: Vec<String>,
  pub file: Option<String>,
  pub content: String,
  pub patch: String,
  pub error: Option<String>,
  pub busy: bool,
  pub browser_frame: Option<(u32, u32, u64)>,
  pub browser_human_control: bool,
  pub browser_error: Option<String>,
  pub browser_tab: Option<String>,
  pub browser_space: Option<String>,
  pub browser_viewport: Option<(u32, u32)>,
  pub browser_input_pending: bool,
  pub browser_queued_inputs: usize,
  pub browser_inputs_sent: u64,
  pub browser_pointer: Option<(i32, i32)>,
}

impl Desktop {
  pub fn composer_input_id(&self) -> ElementId {
    ("input", self.composer.entity_id()).into()
  }
  pub fn snapshot(&self, cx: &App) -> DesktopSnapshot {
    DesktopSnapshot {
      sidebar_expanded: self.show_sidebar,
      workbench_expanded: self.show_workbench,
      settings_open: self.show_settings,
      local_state_loading: self.persistence_loading,
      local_state_loaded: self.persistence_loaded,
      layout_sizes: self
        .main_layout
        .read(cx)
        .sizes()
        .iter()
        .map(|size| size.as_f32())
        .collect(),
      tabs: self.chat_tab_snapshots(cx),
      status: self.vm.status.clone(),
      session_titles: self
        .vm
        .sessions
        .iter()
        .map(|session| session.title.clone())
        .collect(),
      selected_session: self.vm.selected.clone(),
      active_run: self.vm.active_run.clone(),
      event_cursor: self.vm.cursor,
      draft: self.composer.read(cx).value().to_string(),
      messages: self
        .vm
        .conversation
        .iter()
        .map(|message| {
          (
            message.role.to_string(),
            message
              .markdown
              .read(cx)
              .rendered_text()
              .source()
              .to_string(),
          )
        })
        .collect(),
      approvals: self
        .vm
        .approvals
        .iter()
        .map(|approval| {
          (
            approval.id.clone(),
            approval.tool.clone(),
            approval.arguments_json.clone(),
          )
        })
        .collect(),
      error: self.vm.error.clone(),
      busy: self.vm.busy,
      workbench: {
        let state = self.tasks.read(cx);
        WorkbenchSnapshot {
          task_pending: state.task_pending,
          computer: state.computer.read(cx).snapshot(cx),
          paths: state.files.iter().map(|file| file.path.clone()).collect(),
          file: state.file.as_ref().map(|file| file.path.clone()),
          content: state.editor.read(cx).value().to_string(),
          patch: state.patch.clone(),
          error: state.error.clone(),
          busy: state.busy,
          browser_frame: state
            .frame
            .as_ref()
            .map(|(frame, _)| (frame.width, frame.height, frame.sequence)),
          browser_error: state.browser_error.clone(),
          browser_tab: state.tab.as_ref().map(|tab| tab.id.clone()),
          browser_space: state.space.as_ref().map(|space| space.id.clone()),
          browser_viewport: state.requested_viewport,
          browser_input_pending: state.input_pending || state.input_frame_pending,
          browser_queued_inputs: state.input_queue.len(),
          browser_inputs_sent: state.inputs_sent,
          browser_pointer: state.last_pointer,
          browser_human_control: state
            .space
            .as_ref()
            .is_some_and(|space| space.human_control),
        }
      },
    }
  }
}

/// Presentation-only ownership and selection; contains no native handles or credentials.
#[derive(Clone)]
pub struct ComputerSnapshot {
  pub available: bool,
  pub pending: bool,
  pub selected_session: Option<String>,
  pub bound_session: Option<String>,
  pub bound_scope: Option<String>,
  pub targets: Vec<String>,
  pub selected_scope: Option<String>,
  pub attached: bool,
  pub detach_warning: bool,
  pub error: Option<String>,
}

impl std::fmt::Debug for ComputerSnapshot {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("ComputerSnapshot")
      .field("available", &self.available)
      .field("pending", &self.pending)
      .field("selected_session", &self.selected_session)
      .field("bound_session", &self.bound_session)
      .field("attached", &self.attached)
      .field("detach_warning", &self.detach_warning)
      .field("error", &self.error)
      .finish_non_exhaustive()
  }
}

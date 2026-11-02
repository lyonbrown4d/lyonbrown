use super::dispatch::TaskDispatchContext;
use super::state::Workbench;
use agent_client::AgentClient;
use agent_protocol::v1::Run;
use gpui_kit::*;

impl Workbench {
  pub fn bind(
    &mut self,
    client: Option<AgentClient>,
    session: Option<String>,
    runs: Vec<Run>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    self
      .git
      .update(cx, |git, cx| git.bind(client.clone(), session.clone(), cx));
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
}

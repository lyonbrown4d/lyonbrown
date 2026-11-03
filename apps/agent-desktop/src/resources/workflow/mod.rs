//! Presents daemon-owned progress and reviewed checkpoints; contains no execution authority.
mod commands;
mod plan_actions;
mod view;
use agent_client::AgentClient;
use agent_protocol::v1;
use gpui_kit::{
  component::{input::InputState, text::TextViewState},
  *,
};
use std::sync::Arc;

pub(crate) struct WorkflowPane {
  runtime: Arc<tokio::runtime::Runtime>,
  client: Option<AgentClient>,
  session: Option<String>,
  generation: u64,
  pub(in crate::resources) pending: bool,
  busy: bool,
  plans: Vec<v1::WorkflowPlan>,
  changes: Vec<v1::WorkflowChange>,
  plan_after: String,
  change_after: String,
  plan: Option<v1::WorkflowPlan>,
  change: Option<v1::WorkflowChange>,
  title: Entity<InputState>,
  step_title: Entity<InputState>,
  acceptance: Entity<InputState>,
  report: Entity<InputState>,
  operation: Entity<InputState>,
  diff: Entity<TextViewState>,
  message: String,
  sender: async_channel::Sender<Completion>,
  _completion: Task<()>,
}
struct Completion {
  generation: u64,
  mutation: bool,
  result: anyhow::Result<Reply>,
}
enum Reply {
  Inventory(v1::WorkflowPlans, v1::WorkflowChanges),
  Plans(v1::WorkflowPlans),
  Changes(v1::WorkflowChanges),
  Plan(v1::WorkflowPlan),
  Change(v1::WorkflowChange),
}
impl WorkflowPane {
  pub(in crate::resources) fn new(
    runtime: Arc<tokio::runtime::Runtime>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let (sender, receiver) = crate::completion::channel::<Completion>(16);
    let completion = cx.spawn_in(window, async move |view, cx| {
      while let Ok(completion) = receiver.recv().await {
        if view.update_in(cx, |view, window, cx| {
          if completion.mutation { view.pending = false; }
          if completion.generation == view.generation {
            view.busy = false;
            match completion.result {
              Ok(Reply::Inventory(plans, changes)) => { view.plans = plans.plans; view.plan_after = plans.next_after;
                view.changes = changes.changes; view.change_after = changes.next_after; },
              Ok(Reply::Plans(page)) => { view.plans.extend(page.plans); view.plan_after = page.next_after; },
              Ok(Reply::Changes(page)) => { view.changes.extend(page.changes); view.change_after = page.next_after; },
              Ok(Reply::Plan(plan)) => { view.plan = Some(plan); view.message = "Plan state refreshed from the daemon.".into(); },
              Ok(Reply::Change(change)) => {
                let source = change.files.iter().map(|file| format!("{}\n{}", file.path, file.unified_diff)).collect::<Vec<_>>().join("\n");
                // A fence longer than any source run keeps untrusted diff text in a code block.
                let longest = source.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                view.diff = cx.new(|cx| TextViewState::markdown(format!("{fence}diff\n{source}\n{fence}"), cx));
                view.change = Some(change); view.message = "Checkpoint loaded. Review the exact diff before applying.".into();
              },
              Err(_) => view.message = "Request did not complete. Refresh to reconcile state before retrying; changes were not replayed.".into(),
            }
          }
          cx.notify();
        }).is_err() { break; }
      }
    });
    Self {
      runtime,
      client: None,
      session: None,
      generation: 0,
      pending: false,
      busy: false,
      plans: vec![],
      changes: vec![],
      plan_after: String::new(),
      change_after: String::new(),
      plan: None,
      change: None,
      title: cx.new(|cx| InputState::new(window, cx).placeholder("Plan title")),
      step_title: cx.new(|cx| InputState::new(window, cx).placeholder("First step")),
      acceptance: cx.new(|cx| InputState::new(window, cx).placeholder("Acceptance criterion")),
      report: cx.new(|cx| {
        InputState::new(window, cx)
          .placeholder("Progress report or explicit user completion explanation")
      }),
      operation: cx.new(|cx| {
        InputState::new(window, cx).placeholder("Host execute_command operation ID to verify")
      }),
      diff: cx
        .new(|cx| TextViewState::markdown("Select a checkpoint to inspect its exact diff.", cx)),
      message: String::new(),
      sender,
      _completion: completion,
    }
  }
  pub(in crate::resources) fn bind(
    &mut self,
    client: Option<AgentClient>,
    session: Option<String>,
    cx: &mut Context<Self>,
  ) {
    self.generation = self.generation.saturating_add(1);
    self.client = client;
    self.session = session;
    self.busy = false;
    self.plans.clear();
    self.changes.clear();
    self.plan = None;
    self.change = None;
    self.plan_after.clear();
    self.change_after.clear();
    self.message.clear();
    self.refresh(cx);
    cx.notify();
  }
  fn request<F>(&mut self, mutation: bool, future: F, cx: &mut Context<Self>)
  where
    F: std::future::Future<Output = anyhow::Result<Reply>> + Send + 'static,
  {
    if self.busy || self.pending || self.client.is_none() || self.session.is_none() {
      return;
    }
    self.busy = true;
    self.pending = mutation;
    let sender = self.sender.clone();
    let generation = self.generation;
    self.runtime.spawn(async move {
      let result = future.await;
      let _ = sender
        .send(Completion {
          generation,
          mutation,
          result,
        })
        .await;
    });
    cx.notify();
  }
}

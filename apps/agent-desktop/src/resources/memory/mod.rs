//! Memory management renders daemon-owned records and submits authenticated commands.
mod commands;
mod view;
use agent_client::AgentClient;
use agent_protocol::v1;
use gpui_kit::{component::input::InputState, *};
use std::sync::Arc;

pub(crate) struct MemoryPanel {
  runtime: Arc<tokio::runtime::Runtime>,
  client: Option<AgentClient>,
  session: Option<String>,
  generation: u64,
  busy: bool,
  pub(in crate::resources) pending: bool,
  items: Vec<v1::MemoryRecord>,
  next_cursor: String,
  selected: Option<v1::MemoryRecord>,
  draft_id: String,
  draft_scope: v1::MemoryScopeKind,
  draft_kind: v1::MemoryKind,
  content: Entity<InputState>,
  query: Entity<InputState>,
  reset_inputs: bool,
  status: Option<v1::MemoryStatus>,
  rebuilds: Vec<v1::MemoryRebuild>,
  jobs: Vec<v1::MemoryJob>,
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
  Snapshot(v1::Memories, v1::MemoryStatus, v1::MemoryRebuilds),
  List(v1::Memories, bool),
  Jobs(Vec<v1::MemoryJob>),
  Changed(String),
  Forgotten,
}
impl MemoryPanel {
  pub(in crate::resources) fn new(
    runtime: Arc<tokio::runtime::Runtime>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let (sender, receiver) = crate::completion::channel::<Completion>(16);
    let completion = cx.spawn_in(window, async move |view, cx| {
      while let Ok(completion) = receiver.recv().await {
        if view
          .update_in(cx, |view, _, cx| {
            if completion.mutation {
              view.pending = false;
            }
            if completion.generation == view.generation {
              view.busy = false;
              match completion.result {
                Err(_) => {
                  view.message =
                    "The operation did not complete. Refresh to read the current state.".into()
                }
                Ok(Reply::Snapshot(page, status, rebuilds)) => {
                  view.items = page.items;
                  view.next_cursor = page.next_cursor;
                  view.status = Some(status);
                  view.rebuilds = rebuilds.items;
                  let selected_id = view
                    .selected
                    .as_ref()
                    .map_or_else(|| view.draft_id.clone(), |item| item.id.clone());
                  view.selected = view
                    .items
                    .iter()
                    .find(|item| item.id == selected_id)
                    .cloned();
                }
                Ok(Reply::List(page, append)) => {
                  if append {
                    view.items.extend(page.items);
                  } else {
                    view.items = page.items;
                  }
                  view.next_cursor = page.next_cursor;
                }
                Ok(Reply::Jobs(jobs)) => view.jobs = jobs,
                Ok(Reply::Changed(message)) => {
                  view.refresh(cx);
                  view.message = message;
                }
                Ok(Reply::Forgotten) => {
                  view.selected = None;
                  view.jobs.clear();
                  view.reset_inputs = true;
                  view.draft_id = uuid::Uuid::now_v7().to_string();
                  view.refresh(cx);
                  view.message =
                    "Memory content was forgotten and index removal was queued.".into();
                }
              }
            } else if completion.mutation && !view.busy {
              view.refresh(cx);
            }
            cx.notify();
          })
          .is_err()
        {
          break;
        }
      }
    });
    Self {
      runtime,
      client: None,
      session: None,
      generation: 0,
      busy: false,
      pending: false,
      items: vec![],
      next_cursor: String::new(),
      selected: None,
      draft_id: uuid::Uuid::now_v7().to_string(),
      draft_scope: v1::MemoryScopeKind::User,
      draft_kind: v1::MemoryKind::Fact,
      content: cx.new(|cx| InputState::new(window, cx).placeholder("Memory content")),
      query: cx.new(|cx| InputState::new(window, cx).placeholder("Search active memories")),
      reset_inputs: false,
      status: None,
      rebuilds: vec![],
      jobs: vec![],
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
    let changed = self.client.is_some() != client.is_some() || self.session != session;
    if !changed {
      self.client = client;
      return;
    }
    self.generation = self.generation.saturating_add(1);
    self.client = client;
    self.session = session;
    self.busy = false;
    self.items.clear();
    self.jobs.clear();
    self.rebuilds.clear();
    self.status = None;
    self.selected = None;
    self.next_cursor.clear();
    self.message.clear();
    self.reset_inputs = true;
    self.draft_id = uuid::Uuid::now_v7().to_string();
    self.draft_scope = if self.session.is_some() {
      v1::MemoryScopeKind::Session
    } else {
      v1::MemoryScopeKind::User
    };
    self.refresh(cx);
    cx.notify();
  }
  fn select(&mut self, item: v1::MemoryRecord, window: &mut Window, cx: &mut Context<Self>) {
    if self.busy || self.pending {
      return;
    }
    self.content.update(cx, |input, cx| {
      input.set_value(item.content.clone(), window, cx)
    });
    let id = item.id.clone();
    self.selected = Some(item);
    self.jobs.clear();
    let Some(client) = self.client.clone() else {
      return;
    };
    let session = self.session.clone().unwrap_or_default();
    self.request(
      false,
      async move { Ok(Reply::Jobs(client.memory_jobs(&session, &id).await?.jobs)) },
      cx,
    );
  }
  fn fresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.selected = None;
    self.jobs.clear();
    self.draft_id = uuid::Uuid::now_v7().to_string();
    self
      .content
      .update(cx, |input, cx| input.set_value("", window, cx));
    cx.notify();
  }
}

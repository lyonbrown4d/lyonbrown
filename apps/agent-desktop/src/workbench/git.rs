//! Read-only daemon-owned Git state and component-rendered patches.
use agent_client::AgentClient;
use agent_protocol::v1::GitStatus;
mod view;
use gpui_kit::component::diff::{DiffFile, DiffState};
use gpui_kit::*;
use std::sync::Arc;

enum Reply {
  Status(GitStatus),
  Patch(String, String),
}

pub(crate) struct GitPane {
  runtime: Arc<tokio::runtime::Runtime>,
  sender: async_channel::Sender<(u64, anyhow::Result<Reply>)>,
  client: Option<AgentClient>,
  session: Option<String>,
  generation: u64,
  status: Option<GitStatus>,
  path: String,
  patch: String,
  diff: Entity<DiffState>,
  busy: bool,
  error: Option<String>,
  _completion: Task<()>,
}

impl GitPane {
  pub(super) fn new(
    runtime: Arc<tokio::runtime::Runtime>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let (sender, receiver) = crate::completion::channel::<(u64, anyhow::Result<Reply>)>(16);
    let completion = cx.spawn_in(window, async move |view, cx| {
      while let Ok((generation, result)) = receiver.recv().await {
        if view
          .update_in(cx, |view: &mut Self, _, cx| {
            if view.generation != generation {
              return;
            }
            view.busy = false;
            match result {
              Ok(Reply::Status(status)) => {
                view.status = Some(status);
                view.path.clear();
                view.patch.clear();
                view
                  .diff
                  .update(cx, |diff, cx| diff.set_files(Vec::new(), cx));
              }
              Ok(Reply::Patch(path, patch)) => match DiffFile::parse(&patch) {
                Ok(files) => {
                  view.path = path;
                  view.patch = patch;
                  view.diff.update(cx, |diff, cx| diff.set_files(files, cx));
                }
                Err(error) => view.error = Some(format!("Cannot render Git patch: {error}")),
              },
              Err(error) => view.error = Some(error.to_string()),
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
      sender,
      client: None,
      session: None,
      generation: 0,
      status: None,
      path: String::new(),
      patch: String::new(),
      diff: cx.new(|cx| DiffState::new(Vec::new(), cx)),
      busy: false,
      error: None,
      _completion: completion,
    }
  }

  pub(super) fn bind(
    &mut self,
    client: Option<AgentClient>,
    session: Option<String>,
    cx: &mut Context<Self>,
  ) {
    let changed = self.session != session || self.client.is_some() != client.is_some();
    self.client = client;
    if !changed {
      return;
    }
    self.session = session;
    self.generation = self.generation.saturating_add(1);
    self.status = None;
    self.path.clear();
    self.patch.clear();
    self.error = None;
    self.busy = false;
    self
      .diff
      .update(cx, |diff, cx| diff.set_files(Vec::new(), cx));
    // Query only when requested by the user; selecting a session has no Git side effects.
    cx.notify();
  }

  pub(crate) fn snapshot(&self, cx: &App) -> crate::presentation::GitSnapshot {
    crate::presentation::GitSnapshot {
      session: self.session.clone(),
      status: self.status.clone(),
      path: self.path.clone(),
      patch: self.patch.clone(),
      rendered_files: self.diff.read(cx).files().len(),
      busy: self.busy,
      error: self.error.clone(),
    }
  }

  fn request<F>(&mut self, future: F, cx: &mut Context<Self>)
  where
    F: std::future::Future<Output = anyhow::Result<Reply>> + Send + 'static,
  {
    if self.busy {
      return;
    }
    self.busy = true;
    self.error = None;
    let sender = self.sender.clone();
    let generation = self.generation;
    self.runtime.spawn(async move {
      let _ = sender.send((generation, future.await)).await;
    });
    cx.notify();
  }

  fn refresh(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.request(
      async move { Ok(Reply::Status(client.git_status(&session).await?)) },
      cx,
    );
  }

  fn patch(&mut self, path: String, cx: &mut Context<Self>) {
    if self.busy {
      return;
    }
    self.path.clear();
    self.patch.clear();
    self
      .diff
      .update(cx, |diff, cx| diff.set_files(Vec::new(), cx));
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.request(
      async move {
        let patch = client.workspace_diff(&session, &path).await?;
        Ok(Reply::Patch(path, patch))
      },
      cx,
    );
  }
}

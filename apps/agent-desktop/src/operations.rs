use crate::{
  connection::{Update, public_error},
  shell::Desktop,
  tabs::{ScopedUpdate, TabSender},
};
use agent_client::AgentClient;
use gpui_kit::*;

impl Desktop {
  pub(super) fn connect(&mut self, cx: &mut Context<Self>) {
    if self.tasks.read(cx).pending_browser_operation {
      self.error = Some(crate::tabs::BROWSER_WAIT_MESSAGE.into());
      cx.notify();
      return;
    }
    if self.any_active_work() || self.tasks.read(cx).blocks_connection(cx) {
      return;
    }
    self.navigation.connecting = true;
    self.tasks.update(cx, |tasks, cx| {
      tasks.connection_pending = true;
      cx.notify();
    });
    self.error = None;
    self.status = "Connecting to daemon…".into();
    let endpoint = self.endpoint.read(cx).value().to_string();
    let token = self.token.read(cx).value().to_string();
    let sender = self.sender.clone();
    let connector = self.connector.clone();
    self.runtime.spawn(async move {
      let result = async {
        let client = connector.connect(&endpoint, &token).await?;
        let sessions = client.list_sessions().await?;
        anyhow::Ok(Update::Connected(Box::new(client), sessions))
      }
      .await;
      let _ = sender.send(ScopedUpdate::Connection(
        result.unwrap_or_else(|error| Update::Error(public_error(error))),
      ));
    });
    cx.notify();
  }

  pub(super) fn create_session(&mut self, cx: &mut Context<Self>) {
    if self.navigation.connecting || self.busy {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    self.busy = true;
    self.navigation.pending_requests += 1;
    let workspace = self.workspace.read(cx).value().to_string();
    let sender = self.scoped_sender();
    self.runtime.spawn(async move {
      let update = client
        .create_session("New chat", &workspace)
        .await
        .map(|session| Update::Session(session, false))
        .unwrap_or_else(|error| Update::Error(public_error(error)));
      let _ = sender.send(update);
      sender.finish_request();
    });
    cx.notify();
  }

  pub(super) fn select_session(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
    if self.navigation.connecting || !self.can_change_chat(cx) {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    let existing = self
      .chat_tab_snapshots(cx)
      .into_iter()
      .find(|tab| tab.session_id.as_deref() == Some(id.as_str()));
    if let Some(tab) = existing {
      self.activate_chat_tab(tab.id, window, cx);
      return;
    }
    self.open_session_tab(window, cx);
    self.selected = Some(id.clone());
    self.bind_active_chat(window, cx);
    self.conversation.clear();
    self.runs.clear();
    self.approvals.clear();
    self.error = None;
    self.busy = true;
    let sender = self.scoped_sender();
    self.runtime.spawn(async move {
      let update = client
        .list_runs(&id)
        .await
        .map(Update::Runs)
        .unwrap_or_else(|error| Update::Error(public_error(error)));
      let _ = sender.send(update);
    });
    cx.notify();
  }

  pub(super) fn send(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
    if self.navigation.connecting || self.busy || self.active_run.is_some() {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    let prompt = self.composer.read(cx).value().to_string();
    if prompt.trim().is_empty() {
      return;
    }
    self.busy = true;
    self.navigation.pending_requests += 1;
    self.error = None;
    let session = self.selected.clone();
    let workspace = self.workspace.read(cx).value().to_string();
    let sender = self.scoped_sender();
    let key = uuid::Uuid::now_v7().to_string();
    self.runtime.spawn(async move {
      let result = async {
        let session_id = match session {
          Some(id) => id,
          None => {
            let title = prompt.chars().take(48).collect::<String>();
            let session = client.create_session(&title, &workspace).await?;
            let id = session.id.clone();
            let _ = sender.send(Update::Session(session, true));
            id
          }
        };
        let run = client.start_run(&session_id, &prompt, &key).await?;
        let id = run.id.clone();
        let _ = sender.send(Update::Started(run));
        let watcher = sender.clone();
        sender.attach_watch(tokio::spawn(async move {
          if let Err(error) = watch(client, id, watcher.clone(), 0).await {
            let _ = watcher.send(Update::Error(public_error(error)));
          }
        }));
        anyhow::Ok(())
      }
      .await;
      if let Err(error) = result {
        let _ = sender.send(Update::Error(format!(
          "{} No request was replayed; inspect the run before retrying.",
          public_error(error)
        )));
      }
      sender.finish_request();
    });
    cx.notify();
  }

  pub(super) fn cancel_run(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(id)) = (self.client.clone(), self.active_run.clone()) else {
      return;
    };
    let sender = self.scoped_sender();
    self.runtime.spawn(async move {
      let update = client
        .cancel_run(&id)
        .await
        .map(Update::Run)
        .unwrap_or_else(|error| Update::Error(public_error(error)));
      let _ = sender.send(update);
    });
    cx.notify();
  }

  pub(super) fn inspect_run(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(id)) = (self.client.clone(), self.active_run.clone()) else {
      return;
    };
    let sender = self.scoped_sender();
    let cursor = self.cursor;
    self.error = None;
    if let Some(task) = self.watch_task.take() {
      task.abort();
    }
    self.watch_task = Some(self.runtime.spawn(async move {
      let result = async {
        let run = client.get_run(&id).await?;
        let completed = terminal(&run.status);
        let _ = sender.send(Update::Run(run));
        let _ = sender.send(Update::Approvals(client.list_approvals(&id).await?));
        if !completed {
          watch(client, id, sender.clone(), cursor).await?;
        }
        anyhow::Ok(())
      }
      .await;
      if let Err(error) = result {
        let _ = sender.send(Update::Error(public_error(error)));
      }
    }));
    cx.notify();
  }

  pub(super) fn approve(&mut self, id: String, approved: bool, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    self.approvals.retain(|approval| approval.id != id);
    let sender = self.scoped_sender();
    let run = self.active_run.clone();
    self.runtime.spawn(async move {
      let result = async {
        client.approve(&id, approved).await?;
        if let Some(run) = run {
          let _ = sender.send(Update::Approvals(client.list_approvals(&run).await?));
        }
        anyhow::Ok(())
      }
      .await;
      if let Err(error) = result {
        let _ = sender.send(Update::Error(public_error(error)));
      }
    });
    cx.notify();
  }
}

pub(super) fn terminal(status: &str) -> bool {
  matches!(
    status.to_lowercase().as_str(),
    "completed" | "failed" | "cancelled" | "uncertain"
  )
}

async fn watch(
  client: AgentClient,
  id: String,
  sender: TabSender,
  after_cursor: u64,
) -> anyhow::Result<()> {
  let mut stream = client.watch_run(&id, after_cursor).await?;
  while let Some(event) = stream.message().await? {
    let approval_required = event.kind == "approval_required";
    let completed = matches!(
      event.kind.as_str(),
      "run_completed" | "run_failed" | "run_cancelled" | "run_uncertain"
    );
    let _ = sender.send(Update::Event(event));
    if approval_required {
      let _ = sender.send(Update::Approvals(client.list_approvals(&id).await?));
    }
    if completed {
      let _ = sender.send(Update::Run(client.get_run(&id).await?));
      return Ok(());
    }
  }
  let run = client.get_run(&id).await?;
  let _ = sender.send(Update::Run(run));
  Ok(())
}

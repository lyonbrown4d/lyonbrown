//! Chat-side commands towards agentd: connect, session creation and selection,
//! run submission, cancellation, inspection and approvals. The run event stream
//! lives in `operations::watch`.

use crate::{
  connection::{Update, public_error},
  shell::Desktop,
  tabs::ScopedUpdate,
};
use gpui_kit::*;

mod submit;
mod watch;
pub(crate) use watch::terminal;
use watch::watch;

impl Desktop {
  pub(super) fn connect(&mut self, cx: &mut Context<Self>) {
    if !self.can_change_chat(cx) {
      return;
    }
    if self.any_active_work()
      || self.tasks.read(cx).blocks_connection(cx)
      || self.resources.blocks_connection(cx)
    {
      return;
    }
    self.navigation.connecting = true;
    self.resources.invalidate(cx);
    self.tasks.update(cx, |tasks, cx| {
      tasks.connection_pending = true;
      tasks.computer.update(cx, |computer, cx| {
        computer.begin_connection(cx);
      });
      cx.notify();
    });
    self.vm.error = None;
    self.vm.status = "Connecting to daemon…".into();
    let endpoint = self.endpoint.read(cx).value().to_string();
    let token = self.token.read(cx).value().to_string();
    let sender = self.sender.clone();
    let connector = self.connector.clone();
    self.runtime.spawn(async move {
      let result = async {
        let client = connector.connect(&endpoint, &token).await?;
        let sessions = client.list_sessions().await?;
        let node_id = client.node().await?.id;
        anyhow::Ok(Update::Connected {
          client: Box::new(client),
          sessions,
          endpoint,
          node_id,
        })
      }
      .await;
      let _ = sender
        .send(ScopedUpdate::Connection(
          result.unwrap_or_else(|error| Update::Error(public_error(error))),
        ))
        .await;
    });
    cx.notify();
  }

  pub(super) fn create_session(&mut self, cx: &mut Context<Self>) {
    if self.navigation.connecting || self.persistence_loading || self.vm.busy {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    self.vm.busy = true;
    self.navigation.pending_requests += 1;
    let workspace = self.workspace.read(cx).value().to_string();
    let sender = self.scoped_sender();
    self.runtime.spawn(async move {
      let update = client
        .create_session("New chat", &workspace)
        .await
        .map(|session| Update::Session(session, false))
        .unwrap_or_else(|error| Update::Error(public_error(error)));
      let _ = sender.send(update).await;
      sender.finish_request().await;
    });
    cx.notify();
  }

  pub(super) fn select_session(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
    if !self.can_change_chat(cx) {
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
    if !self.open_session_tab(window, cx) {
      return;
    }
    self.vm.selected = Some(id.clone());
    self.bind_active_chat(window, cx);
    self.vm.conversation.clear();
    self.vm.runs.clear();
    self.vm.approvals.clear();
    self.vm.error = None;
    self.vm.busy = true;
    let sender = self.scoped_sender();
    self.runtime.spawn(async move {
      let update = client
        .list_runs(&id)
        .await
        .map(Update::Runs)
        .unwrap_or_else(|error| Update::Error(public_error(error)));
      let _ = sender.send(update).await;
    });
    cx.notify();
  }

  pub(super) fn cancel_run(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(id)) = (self.client.clone(), self.vm.active_run.clone()) else {
      return;
    };
    let sender = self.scoped_sender();
    self.runtime.spawn(async move {
      let update = client
        .cancel_run(&id)
        .await
        .map(Update::Run)
        .unwrap_or_else(|error| Update::Error(public_error(error)));
      let _ = sender.send(update).await;
    });
    cx.notify();
  }

  pub(super) fn inspect_run(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(id)) = (self.client.clone(), self.vm.active_run.clone()) else {
      return;
    };
    let sender = self.scoped_sender();
    let cursor = self.vm.cursor;
    self.vm.error = None;
    if let Some(task) = self.watch_task.take() {
      task.abort();
    }
    self.watch_task = Some(self.runtime.spawn(async move {
      let result = async {
        let run = client.get_run(&id).await?;
        let completed = terminal(&run.status);
        let _ = sender.send(Update::Run(run)).await;
        let _ = sender
          .send(Update::Approvals(client.list_approvals(&id).await?))
          .await;
        if !completed {
          watch(client, id, sender.clone(), cursor).await?;
        }
        anyhow::Ok(())
      }
      .await;
      if let Err(error) = result {
        let _ = sender.send(Update::Error(public_error(error))).await;
      }
    }));
    cx.notify();
  }

  pub(super) fn approve(&mut self, id: String, approved: bool, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    self.vm.approvals.retain(|approval| approval.id != id);
    let sender = self.scoped_sender();
    let run = self.vm.active_run.clone();
    self.runtime.spawn(async move {
      let result = async {
        client.approve(&id, approved).await?;
        if let Some(run) = run {
          let _ = sender
            .send(Update::Approvals(client.list_approvals(&run).await?))
            .await;
        }
        anyhow::Ok(())
      }
      .await;
      if let Err(error) = result {
        let _ = sender.send(Update::Error(public_error(error))).await;
      }
    });
    cx.notify();
  }
}

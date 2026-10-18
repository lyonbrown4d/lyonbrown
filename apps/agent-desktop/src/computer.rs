//! Explicit interactive-host ownership; chat selection never grants a scope.
mod lifecycle;
mod view;

use crate::{ComputerPort, presentation::ComputerSnapshot};
use agent_client::AgentClient;
use agent_computer_runtime::{DesktopScope, DesktopTarget, RuntimeAttachment};
use gpui_kit::component::select::SelectState;
use gpui_kit::*;
use std::{
  sync::{Arc, mpsc},
  time::Duration,
};

pub(crate) enum Reply {
  Targets(Vec<DesktopTarget>),
  Attached(String, DesktopTarget, RuntimeAttachment),
  Detached(Result<(), ()>),
  Error(&'static str),
}

pub(crate) struct ComputerPane {
  runtime: Arc<tokio::runtime::Runtime>,
  port: fluxdi::Shared<ComputerPort>,
  client: Option<AgentClient>,
  session: Option<String>,
  pub(crate) generation: u64,
  pub(crate) sender: mpsc::Sender<(u64, Reply)>,
  targets: Vec<DesktopTarget>,
  select: Entity<SelectState<Vec<String>>>,
  attachment: Option<RuntimeAttachment>,
  bound_session: Option<String>,
  bound_scope: Option<String>,
  pub(crate) pending: bool,
  pub(crate) connection_pending: bool,
  error: Option<String>,
  detach_warning: bool,
  operation: Option<tokio::task::JoinHandle<()>>,
  _selection: Subscription,
  _poll: Task<()>,
}

impl ComputerPane {
  pub(crate) fn new(
    runtime: Arc<tokio::runtime::Runtime>,
    port: fluxdi::Shared<ComputerPort>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let select =
      cx.new(|cx| SelectState::new(Vec::<String>::new(), None, window, cx).searchable(true));
    let selection = cx.observe(&select, |_, _, cx| cx.notify());
    let (sender, receiver) = mpsc::channel();
    let poll = cx.spawn_in(window, async move |view, cx| {
      loop {
        cx.background_executor().timer(Duration::from_millis(80)).await;
        if view.update_in(cx, |view, window, cx| {
          for (generation, reply) in receiver.try_iter() {
            if generation == view.generation {
              view.apply(reply, window, cx);
            }
            // An obsolete Attached reply is dropped here, cancelling its host.
          }
          let failure = view.attachment.as_ref().is_some_and(|host| host.is_finished());
          if failure && view.error.is_none() {
            view.error = Some("Computer runtime disconnected. Detach before explicitly attaching again; actions were not replayed.".into());
            cx.notify();
          }
        }).is_err() {
          break;
        }
      }
    });
    Self {
      runtime,
      port,
      client: None,
      session: None,
      generation: 0,
      sender,
      targets: Vec::new(),
      select,
      attachment: None,
      bound_session: None,
      bound_scope: None,
      pending: false,
      connection_pending: false,
      error: None,
      detach_warning: false,
      operation: None,
      _selection: selection,
      _poll: poll,
    }
  }

  fn request(&mut self, future: impl std::future::Future<Output = Reply> + Send + 'static) {
    self.generation = self.generation.saturating_add(1);
    self.pending = true;
    if !self.detach_warning {
      self.error = None;
    }
    let generation = self.generation;
    let sender = self.sender.clone();
    self.operation = Some(self.runtime.spawn(async move {
      let _ = sender.send((generation, future.await));
    }));
  }

  fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.pending
      || self.connection_pending
      || !self.port.0.available()
      || self.client.is_none()
      || self.session.is_none()
      || self.bound_session.is_some()
    {
      return;
    }
    self.invalidate(window, cx);
    let factory = self.port.0.clone();
    self.request(async move {
      match factory.targets().await {
        Ok(targets) => Reply::Targets(targets),
        Err(error) => Reply::Error(agent_computer_runtime::computer_error_message(&error)),
      }
    });
    cx.notify();
  }

  fn selected_target(&self, cx: &App) -> Option<DesktopTarget> {
    let selected = self.select.read(cx).selected_value()?;
    self
      .targets
      .iter()
      .find(|target| target_label(target) == *selected)
      .cloned()
  }

  fn attach(&mut self, cx: &mut Context<Self>) {
    if self.pending
      || self.connection_pending
      || self.bound_session.is_some()
      || !self.port.0.available()
    {
      return;
    }
    let (Some(client), Some(session), Some(target)) = (
      self.client.clone(),
      self.session.clone(),
      self.selected_target(cx),
    ) else {
      return;
    };
    let factory = self.port.0.clone();
    self.request(async move {
      match factory.attach(client, &session, target.clone()).await {
        Ok(host) => Reply::Attached(session, target, host),
        Err(error) => Reply::Error(agent_computer_runtime::computer_error_message(&error)),
      }
    });
    cx.notify();
  }

  fn detach(&mut self, cx: &mut Context<Self>) {
    if self.pending || self.connection_pending {
      return;
    }
    let Some(mut host) = self.attachment.take() else {
      return;
    };
    self.request(async move { Reply::Detached(host.shutdown().await.map_err(|_| ())) });
    cx.notify();
  }

  pub(crate) fn apply(&mut self, reply: Reply, window: &mut Window, cx: &mut Context<Self>) {
    self.pending = false;
    self.operation = None;
    match reply {
      Reply::Targets(targets) => {
        let labels = targets.iter().map(target_label).collect();
        self.targets = targets;
        self.select.update(cx, |select, cx| {
          select.set_selected_index(None, window, cx);
          select.set_items(labels, window, cx);
        });
      }
      Reply::Attached(session, target, host) => {
        if self.session.as_deref() == Some(&session) && self.bound_session.is_none() {
          self.bound_scope = Some(target_label(&target));
          self.bound_session = Some(session);
          self.attachment = Some(host);
          self.detach_warning = false;
          self.error = None;
        }
      }
      Reply::Detached(result) => {
        self.bound_session = None;
        self.bound_scope = None;
        self.detach_warning = result.is_err();
        self.error = result.err().map(|()| "Computer host disconnected before shutdown completed. Native work may still be draining; a new attachment can remain unavailable until it finishes. No actions were replayed.".into());
      }
      Reply::Error(error) => self.error = Some(error.into()),
    }
    cx.notify();
  }

  pub(crate) fn snapshot(&self, cx: &App) -> ComputerSnapshot {
    ComputerSnapshot {
      available: self.port.0.available(),
      pending: self.pending,
      selected_session: self.session.clone(),
      bound_session: self.bound_session.clone(),
      bound_scope: self.bound_scope.clone(),
      targets: self.targets.iter().map(target_label).collect(),
      selected_scope: self.selected_target(cx).as_ref().map(target_label),
      attached: self
        .attachment
        .as_ref()
        .is_some_and(|host| !host.is_finished()),
      error: self.error.clone(),
      detach_warning: self.detach_warning,
    }
  }
}

fn target_label(target: &DesktopTarget) -> String {
  let scope = match &target.scope {
    DesktopScope::Window(id) => format!("Window {id}"),
    DesktopScope::Display(id) => format!("Monitor {id}"),
  };
  format!("{scope} · {}", target.label)
}

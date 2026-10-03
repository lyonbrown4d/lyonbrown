use std::collections::VecDeque;
use std::future;

use agent_core::{AgentError, AgentEvent};
use agent_kernel::{Kernel, KernelControl};
use agent_tool::{ApprovalDecision as ToolDecision, ApprovalResponse};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::approval::PendingApproval;
use crate::{ApprovalInbox, Projector, RuntimeActorError, RuntimeEndpoint};

mod cleanup;

type TurnResult = Result<Vec<AgentEvent>, AgentError>;
type TurnTask = JoinHandle<(Kernel, TurnResult)>;

pub(super) struct Actor {
  pub(super) kernel: Option<Kernel>,
  pub(super) endpoint: RuntimeEndpoint,
  events: broadcast::Receiver<AgentEvent>,
  pub(super) projector: Projector,
  pub(super) queued: VecDeque<String>,
  pub(super) turn_task: Option<TurnTask>,
  pub(super) control: Option<KernelControl>,
  pub(super) cancellation: Option<CancellationToken>,
  approvals: Option<ApprovalInbox>,
  pub(super) pending_approval: Option<PendingApproval>,
  pub(super) commands_open: bool,
  replay_required: bool,
  shutdown: CancellationToken,
}

pub(super) async fn run(
  kernel: Kernel,
  endpoint: RuntimeEndpoint,
  projector: Projector,
  approvals: Option<ApprovalInbox>,
  shutdown: CancellationToken,
) -> Result<(), RuntimeActorError> {
  let events = kernel.subscribe();
  Actor {
    kernel: Some(kernel),
    endpoint,
    events,
    projector,
    queued: VecDeque::new(),
    turn_task: None,
    control: None,
    cancellation: None,
    approvals,
    shutdown,
    pending_approval: None,
    commands_open: true,
    replay_required: false,
  }
  .run()
  .await
}

impl Actor {
  async fn run(mut self) -> Result<(), RuntimeActorError> {
    loop {
      if self.commands_open && self.shutdown.is_cancelled() {
        self.close_commands();
      }
      if self.turn_task.is_none() {
        if !self.commands_open {
          self.close_kernel().await?;
          return Ok(());
        }
        if let Some(text) = self.queued.pop_front() {
          self.publish_queue();
          self.start_turn(text);
          continue;
        }
        tokio::select! {
          biased;
          () = self.shutdown.cancelled() => self.close_commands(),
          request = self.endpoint.recv_request() => match request {
            Some(request) => self.handle_command(request),
            None => self.close_commands(),
          }
        }
        continue;
      }

      tokio::select! {
        () = self.shutdown.cancelled(), if self.commands_open => self.close_commands(),
        request = self.endpoint.recv_request(), if self.commands_open => {
          match request {
            Some(request) => self.handle_command(request),
            None => self.close_commands(),
          }
        }
        event = self.events.recv() => self.project_event(event)?,
        approval = receive_approval(&mut self.approvals) => self.receive_approval(approval)?,
        () = wait_for_approval_deadline(
          self.pending_approval.as_ref().and_then(|pending| pending.request.deadline)
        ) => self.deny_pending(),
        outcome = self.turn_task.as_mut().expect("active turn has a task") => {
          self.finish_turn(outcome)?;
        }
      }
    }
  }

  pub(super) fn start_turn(&mut self, text: String) {
    let mut kernel = self.kernel.take().expect("idle actor owns its kernel");
    self.cancellation = Some(kernel.renew_cancellation());
    kernel.prepare_turn_control();
    self.control = Some(kernel.control());
    self.turn_task = Some(tokio::spawn(async move {
      let result = kernel.run_turn(&text).await;
      (kernel, result)
    }));
    self.projector.set_phase(crate::RuntimePhase::Preparing);
    self.publish();
  }

  pub(super) fn publish(&self) {
    let _ = self.endpoint.publish(self.projector.snapshot().clone());
  }

  pub(super) fn publish_queue(&mut self) {
    self.projector.set_queued_follow_ups(&self.queued);
    self.publish();
  }

  pub(super) fn deny_pending(&mut self) {
    if let Some(pending) = self.pending_approval.take() {
      let request_id = pending.request.request_id;
      pending.respond(ApprovalResponse {
        request_id,
        decision: ToolDecision::Deny,
      });
      self.projector.clear_approval_request(request_id);
      self.publish();
    }
  }

  fn close_commands(&mut self) {
    self.commands_open = false;
    self.endpoint.close_commands();
    self.deny_pending();
    self.queued.clear();
    self.publish_queue();
    if let Some(cancellation) = &self.cancellation {
      cancellation.cancel();
    }
  }

  fn receive_approval(
    &mut self,
    approval: Option<PendingApproval>,
  ) -> Result<(), RuntimeActorError> {
    let Some(approval) = approval else {
      self.approvals = None;
      return Ok(());
    };
    self.drain_events()?;
    if self
      .pending_approval
      .as_ref()
      .is_some_and(PendingApproval::is_closed)
    {
      self.deny_pending();
    }
    let cancelling = self
      .cancellation
      .as_ref()
      .is_some_and(CancellationToken::is_cancelled);
    if !self.commands_open || cancelling || self.pending_approval.is_some() {
      let request_id = approval.request.request_id;
      approval.respond(ApprovalResponse {
        request_id,
        decision: ToolDecision::Deny,
      });
      return Ok(());
    }
    if !self.projector.show_approval_request(&approval.request) {
      let request_id = approval.request.request_id;
      approval.respond(ApprovalResponse {
        request_id,
        decision: ToolDecision::Deny,
      });
      return Ok(());
    }
    self.pending_approval = Some(approval);
    self.publish();
    Ok(())
  }

  fn finish_turn(
    &mut self,
    outcome: Result<(Kernel, TurnResult), tokio::task::JoinError>,
  ) -> Result<(), RuntimeActorError> {
    let (kernel, result) =
      outcome.map_err(|error| RuntimeActorError::TurnTask(error.to_string()))?;
    let was_cancelled = self
      .cancellation
      .as_ref()
      .is_some_and(CancellationToken::is_cancelled);
    self.kernel = Some(kernel);
    self.turn_task = None;
    self.control = None;
    self.cancellation = None;
    self.deny_pending();
    self.drain_events()?;
    if self.replay_required {
      let models = crate::ModelRuntimeView {
        catalog: self.projector.snapshot().model_catalog.clone(),
        policy: self.projector.snapshot().model_policy.clone(),
      };
      self.projector = super::initial_projector(
        self
          .kernel
          .as_ref()
          .expect("finished turn returns its kernel"),
        crate::RuntimePhase::Completed,
        models,
      );
      self.projector.set_queued_follow_ups(&self.queued);
      self.replay_required = false;
      self.publish();
    }
    self.publish();
    if was_cancelled {
      self.projector.cancel_active_turn();
      self.publish();
    } else if let Err(error) = result
      && !matches!(error, AgentError::Cancelled)
      && self.projector.snapshot().phase != crate::RuntimePhase::FailedRecoverable
    {
      self.projector.record_failure(error.to_string());
      self.publish();
    }
    Ok(())
  }

  fn project_event(
    &mut self,
    event: Result<AgentEvent, broadcast::error::RecvError>,
  ) -> Result<(), RuntimeActorError> {
    match event {
      Ok(event) => {
        self.projector.apply(event);
        self.publish();
        Ok(())
      }
      Err(broadcast::error::RecvError::Closed) => Err(RuntimeActorError::EventStreamClosed),
      Err(broadcast::error::RecvError::Lagged(_)) => {
        self.replay_required = true;
        Ok(())
      }
    }
  }

  fn drain_events(&mut self) -> Result<(), RuntimeActorError> {
    loop {
      match self.events.try_recv() {
        Ok(event) => self.projector.apply(event),
        Err(broadcast::error::TryRecvError::Empty) => return Ok(()),
        Err(broadcast::error::TryRecvError::Closed) => {
          return Err(RuntimeActorError::EventStreamClosed);
        }
        Err(broadcast::error::TryRecvError::Lagged(_)) => self.replay_required = true,
      }
    }
  }

  async fn close_kernel(&mut self) -> Result<(), RuntimeActorError> {
    if let Some(kernel) = self.kernel.as_mut() {
      kernel
        .close_storage()
        .await
        .map_err(RuntimeActorError::Kernel)?;
    }
    Ok(())
  }
}

async fn receive_approval(inbox: &mut Option<ApprovalInbox>) -> Option<PendingApproval> {
  match inbox {
    Some(inbox) => inbox.recv().await,
    None => future::pending().await,
  }
}

async fn wait_for_approval_deadline(deadline: Option<std::time::Instant>) {
  match deadline {
    Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
    None => future::pending().await,
  }
}

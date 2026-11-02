use agent_core::TaskEnvelope;
use agent_protocol::v1::{self, orchestration_service_server::OrchestrationService};
use agent_runtime::{RuntimeError, ToolCall};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tonic::Request;

use super::{
  DelegationTool, digest,
  input::{self, Arguments, Operation},
  output,
};

enum Mutation {
  Dispatch(Box<TaskEnvelope>),
  Cancel,
  Accept(bool),
}

impl DelegationTool {
  pub(super) async fn perform(&self, call: ToolCall, cancel: CancellationToken) -> Result<Value> {
    self.fence(&cancel).await?;
    let arguments: Arguments = serde_json::from_value(call.arguments)
      .map_err(|_| anyhow::anyhow!("invalid delegation operation or arguments"))?;
    let target = self.target(arguments.call.target()).await?;
    match arguments.call {
      Operation::Dispatch {
        instructions,
        capabilities,
        timeout_ms,
        ..
      } => {
        let envelope = input::envelope(self, &call.id, instructions, capabilities, timeout_ms)?;
        self.validate_scope(&target.id, &envelope).await?;
        ensure!(
          self.daemon.store.task(&envelope.task_id).await?.is_none(),
          "delegation identity exists; reconcile it without replaying dispatch"
        );
        let exact = json!({"op":"dispatch","target":target_identity(&target),"envelope":envelope});
        self.approve(&exact, &cancel).await?;
        self.revalidate_target(&target).await?;
        self.validate_scope(&target.id, &envelope).await?;
        let id = envelope.task_id.clone();
        self
          .mutate(&id, target, Mutation::Dispatch(Box::new(envelope)), cancel)
          .await
      }
      Operation::Get { task_id, .. } => Ok(output(self.task(&task_id, &target.id).await?)),
      Operation::Reconcile { task_id, .. } => {
        self.task(&task_id, &target.id).await?;
        tokio::select! {
          value = self.daemon.reconcile_task(Request::new(v1::RemoteTaskRequest {
            target: Some(target.clone()), task_id: task_id.clone(),
          })) => value?.into_inner(),
          _ = cancel.cancelled() => anyhow::bail!("delegation observation cancelled"),
        };
        self.fence(&cancel).await?;
        Ok(output(self.task(&task_id, &target.id).await?))
      }
      Operation::Cancel { task_id, .. } => {
        let task = self.task(&task_id, &target.id).await?;
        let exact =
          json!({"op":"cancel","target":target_identity(&target),"envelope":task.envelope});
        self.approve(&exact, &cancel).await?;
        self.task(&task_id, &target.id).await?;
        self.revalidate_target(&target).await?;
        self
          .mutate(&task_id, target, Mutation::Cancel, cancel)
          .await
      }
      Operation::Accept {
        task_id, accepted, ..
      } => {
        self.task(&task_id, &target.id).await?;
        // Reconcile first so approval binds the verified current attempt and result.
        tokio::select! {
          value = self.daemon.reconcile_task(Request::new(v1::RemoteTaskRequest {
            target: Some(target.clone()), task_id: task_id.clone(),
          })) => value?,
          _ = cancel.cancelled() => anyhow::bail!("delegation observation cancelled before approval"),
        };
        self.fence(&cancel).await?;
        let task = self.task(&task_id, &target.id).await?;
        ensure!(
          task.status == "awaiting_acceptance" && task.attempt_status == "succeeded",
          "only a reconciled successful attempt may be accepted or rejected"
        );
        let result_digest = digest(&task);
        let exact = json!({"op":"accept","target":target_identity(&target),"envelope":task.envelope,
          "accepted":accepted,"result_digest":result_digest,
          "untrusted_worker_content":true,"result":task.result,"error":task.error});
        self.approve(&exact, &cancel).await?;
        self.revalidate_target(&target).await?;
        let current = self.task(&task_id, &target.id).await?;
        ensure!(
          digest(&current) == result_digest,
          "delegation result changed after approval"
        );
        self
          .mutate(&task_id, target, Mutation::Accept(accepted), cancel)
          .await
      }
    }
  }

  async fn validate_scope(&self, target: &str, envelope: &TaskEnvelope) -> Result<()> {
    let configured = self
      .daemon
      .config
      .delegation_targets
      .iter()
      .find(|value| value.node_id == target)
      .ok_or_else(|| anyhow::anyhow!("delegation target unavailable"))?;
    let trust = crate::peers::manager(&self.daemon)?.trust(target).await?;
    ensure!(
      envelope
        .capabilities
        .iter()
        .all(|cap| configured.capabilities.contains(cap)
          && trust.capabilities.contains(cap)
          && self.capabilities.contains(cap)),
      "delegation exceeds configured, paired, or execution plan scope"
    );
    ensure!(
      envelope.deadline_ms > agent_core::now_ms(),
      "delegation deadline expired during approval"
    );
    Ok(())
  }

  async fn mutate(
    &self,
    id: &str,
    target: v1::Node,
    operation: Mutation,
    cancel: CancellationToken,
  ) -> Result<Value> {
    self.fence(&cancel).await?;
    let target_id = target.id.clone();
    let action = async {
      match operation {
        Mutation::Dispatch(envelope) => {
          self
            .daemon
            .dispatch_task(Request::new(v1::DispatchTaskRequest {
              target: Some(target),
              envelope: Some((*envelope).into()),
            }))
            .await
        }
        Mutation::Cancel => {
          self
            .daemon
            .cancel_task(Request::new(v1::RemoteTaskRequest {
              target: Some(target),
              task_id: id.into(),
            }))
            .await
        }
        Mutation::Accept(accepted) => {
          self
            .daemon
            .accept_result(Request::new(v1::AcceptRemoteResultRequest {
              target: Some(target),
              task_id: id.into(),
              accepted,
            }))
            .await
        }
      }
    };
    let result = tokio::select! {
      result = action => Some(result),
      _ = cancel.cancelled() => None,
    };
    self.daemon.events.notify_waiters();
    match result {
      Some(Ok(_)) => Ok(output(self.task(id, &target_id).await?)),
      error => {
        if let Some(task) = self.daemon.store.task(id).await? {
          if matches!(task.status.as_str(), "completed" | "rejected") {
            return Ok(output(task));
          }
          // Once intent exists, cancellation/lost responses never invite model retries.
          if matches!(task.attempt_status.as_str(), "accepted" | "running") {
            self
              .daemon
              .store
              .mark_task_uncertain(id, &task.envelope.attempt_id, self.epoch)
              .await
              .map_err(|_| {
                RuntimeError::Uncertain(format!(
                  "delegation interrupted for task {id}; durable outcome requires reconciliation"
                ))
              })?;
          }
          return Err(
            RuntimeError::Uncertain(format!(
              "delegation outcome unknown for task {id}; reconcile this identity without replay"
            ))
            .into(),
          );
        }
        match error {
          Some(Err(error)) => Err(error.into()),
          _ => anyhow::bail!("delegation cancelled before durable dispatch"),
        }
      }
    }
  }
}

fn target_identity(target: &v1::Node) -> Value {
  json!({"id":target.id,"endpoint":target.endpoint,"pairing_fingerprint":target.fingerprint})
}

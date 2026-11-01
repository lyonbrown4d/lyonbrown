use super::super::state::{DispatchIntent, Reply, Workbench};
use agent_protocol::v1::{Node, TaskEnvelope};
use gpui_kit::*;
use std::time::{SystemTime, UNIX_EPOCH};

impl Workbench {
  fn begin_task_request(&mut self) -> u64 {
    self.task_generation = self.task_generation.saturating_add(1);
    self.task_pending = true;
    self.task_generation
  }

  pub(crate) fn refresh_tasks(&mut self, after: String) {
    if self.task_pending {
      return;
    }
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    if after.is_empty() {
      self.tasks.clear();
      self.task_cursor.clear();
    }
    let generation = self.begin_task_request();
    let workbench_generation = self.generation;
    let sender = self.sender.clone();
    let unresolved_only = self.recovery_tasks_only;
    self.runtime.spawn(async move {
      let result = if unresolved_only {
        client.recovery_tasks(&session, &after).await
      } else {
        client.list_tasks(&session, &after, 100).await
      };
      let reply = match result {
        Ok(page) => Reply::Tasks(generation, page.tasks, page.next_task_id),
        Err(error) => Reply::TaskError(generation, format!("Unable to load tasks: {error}"), None),
      };
      let _ = sender.send((workbench_generation, reply)).await;
    });
  }

  pub(super) fn dispatch_task(&mut self, cx: &App) {
    if self.task_pending || self.dispatch_intent.is_some() {
      return;
    }
    let (Some(client), Some(session), Some(parent_run)) = (
      self.client.clone(),
      self.session.clone(),
      self
        .runs
        .iter()
        .find(|run| run.status == "running")
        .cloned(),
    ) else {
      self.error = Some("Select a chat with an active run before dispatching a task.".into());
      return;
    };
    let node_id = self.task_target_id.read(cx).value().to_string();
    let endpoint = self.task_target_endpoint.read(cx).value().to_string();
    let instructions = self.task_instructions.read(cx).value().to_string();
    let scope = self
      .task_capabilities
      .read(cx)
      .selected_value()
      .map(ToString::to_string)
      .unwrap_or_else(|| "Read only".into());
    if node_id.trim().is_empty()
      || !endpoint.starts_with("https://")
      || endpoint.contains('@')
      || instructions.trim().is_empty()
    {
      self.error = Some(
        "Enter a paired Worker node ID, credential-free HTTPS endpoint, and task instructions."
          .into(),
      );
      return;
    }
    let generation = self.begin_task_request();
    let workbench_generation = self.generation;
    let task_id = uuid::Uuid::now_v7().to_string();
    let attempt_id = uuid::Uuid::now_v7().to_string();
    let idempotency_key = format!("desktop-{task_id}");
    self.dispatch_intent = Some(DispatchIntent {
      task_id: task_id.clone(),
      attempt_id: attempt_id.clone(),
      idempotency_key: idempotency_key.clone(),
      uncertain: true,
    });
    let sender = self.sender.clone();
    let prepare_client = client.clone();
    self.runtime.spawn(async move {
      let prepared = async {
        let node = prepare_client.node().await?;
        anyhow::ensure!(node.capabilities.iter().any(|capability| capability == "tasks.manage"), "daemon does not support task management");
        let selected = prepare_client.list_sessions().await?.into_iter().find(|value| value.id == session).ok_or_else(|| anyhow::anyhow!("selected session no longer exists"))?;
        anyhow::ensure!(selected.owner_node_id == node.id, "selected session is not owned by this Master");
        let current = prepare_client.get_run(&parent_run.id).await?;
        anyhow::ensure!(current.id == parent_run.id && current.session_id == session && current.status == "running", "parent run must still be active in the selected session");
        let now_ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
        let envelope = TaskEnvelope {
          task_id: task_id.clone(), attempt_id, parent_run_id: current.id,
          session_id: selected.id, owner_node_id: node.id, owner_epoch: selected.owner_epoch,
          workspace: selected.workspace, instructions,
          capabilities: if scope == "Read and write after approval" {
            vec!["workspace.read".into(), "workspace.write.approval".into()]
          } else {
            vec!["workspace.read".into()]
          }, idempotency_key,
          deadline_ms: now_ms.checked_add(3_600_000).ok_or_else(|| anyhow::anyhow!("task deadline overflow"))?,
        };
        Ok(envelope)
      }.await;
      let reply = match prepared {
        Ok(envelope) => match client.dispatch_task(Node { id: node_id, endpoint, ..Default::default() }, envelope).await {
          Ok(task) => Reply::Task(generation, task),
          Err(error) => {
            let uncertain = !known_pre_dispatch_rejection(&error);
            let guidance = if uncertain {
              "Inspect and reconcile the same task identity; do not dispatch a fresh retry."
            } else {
              "The daemon rejected dispatch before execution; correct the form or explicitly start a separate task."
            };
            Reply::TaskError(generation, format!("Task dispatch {} {guidance} {error}", if uncertain { "outcome is uncertain." } else { "was rejected." }), Some(uncertain))
          },
        },
        Err(error) => Reply::TaskError(generation, format!("Task was not dispatched: {error}. Correct the form or explicitly start a separate task; the original ID is retained."), Some(false)),
      };
      let _ = sender.send((workbench_generation, reply)).await;
    });
  }

  pub(super) fn operate_task(&mut self, task_id: String, operation: TaskOperation, cx: &App) {
    if self.task_pending {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    let node_id = self.task_target_id.read(cx).value().to_string();
    let endpoint = self.task_target_endpoint.read(cx).value().to_string();
    let target_required = !matches!(operation, TaskOperation::Inspect);
    if target_required
      && (node_id.trim().is_empty() || !endpoint.starts_with("https://") || endpoint.contains('@'))
    {
      self.error = Some(
        "Enter the original paired Worker node ID and endpoint to operate on this task.".into(),
      );
      return;
    }
    let target = Node {
      id: node_id,
      endpoint,
      ..Default::default()
    };
    let generation = self.begin_task_request();
    let workbench_generation = self.generation;
    let sender = self.sender.clone();
    self.runtime.spawn(async move {
      let result = match operation {
        TaskOperation::Inspect => client.task(&task_id).await,
        TaskOperation::Reconcile => client.reconcile_task(target, &task_id).await,
        TaskOperation::Cancel => client.cancel_task(target, &task_id).await,
        TaskOperation::Accept(accepted) => client.accept_task_result(target, &task_id, accepted).await,
      };
      let reply = result.map(|task| Reply::Task(generation, task)).unwrap_or_else(|error| Reply::TaskError(generation, format!("Task operation may have an uncertain outcome. Inspect and reconcile the same task ID. {error}"), None));
      let _ = sender.send((workbench_generation, reply)).await;
    });
  }

  pub(super) fn dispatch_separate_task(&mut self, cx: &App) {
    if self.task_pending {
      return;
    }
    if let Some(previous) = self.dispatch_intent.take() {
      self.dispatch_history.push(previous);
    }
    self.dispatch_task(cx);
  }
}

fn known_pre_dispatch_rejection(error: &anyhow::Error) -> bool {
  error.downcast_ref::<tonic::Status>().is_some_and(|status| {
    matches!(
      status.code(),
      tonic::Code::InvalidArgument
        | tonic::Code::Unauthenticated
        | tonic::Code::PermissionDenied
        | tonic::Code::NotFound
    )
  })
}

#[derive(Clone, Copy)]
pub(super) enum TaskOperation {
  Inspect,
  Reconcile,
  Cancel,
  Accept(bool),
}

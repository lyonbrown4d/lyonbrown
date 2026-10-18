use std::{collections::HashMap, path::PathBuf, sync::Arc};

use agent_core::{Session, Task, TaskEnvelope};
use anyhow::{Result, ensure};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use crate::peers::PeerManager;

#[derive(Clone)]
pub struct WorkerJob {
  pub task: TaskEnvelope,
  pub session: Session,
  pub workspace: PathBuf,
}

#[async_trait::async_trait]
pub trait WorkerExecutor: Send + Sync + 'static {
  /// Run one independent local Harness session. Honor cancellation before every side effect.
  async fn execute(&self, job: WorkerJob, cancellation: CancellationToken) -> Result<String>;
}

/// An executor lost confirmation of a side effect; the attempt must never be replayed.
#[derive(Debug)]
pub struct UncertainOutcome;
impl std::fmt::Display for UncertainOutcome {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str("worker execution requires reconciliation")
  }
}
impl std::error::Error for UncertainOutcome {}

#[derive(Clone)]
pub struct TaskWorker {
  pub(crate) manager: PeerManager,
  pub(crate) executor: Arc<dyn WorkerExecutor>,
  pub(crate) workspace_root: PathBuf,
  pub(crate) active: Arc<Mutex<HashMap<String, CancellationToken>>>,
  pub(crate) submission: Arc<Mutex<()>>,
  pub(crate) changed: Arc<Notify>,
}

/// A lifecycle port without an executor reference, safe for the host to retain.
#[derive(Clone)]
pub struct WorkerControl {
  active: Arc<Mutex<HashMap<String, CancellationToken>>>,
  changed: Arc<Notify>,
}

impl WorkerControl {
  pub async fn active_count(&self) -> usize {
    self.active.lock().await.len()
  }

  /// The caller seals submission before calling this bounded cooperative drain.
  pub async fn shutdown(&self) {
    for cancellation in self.active.lock().await.values() {
      cancellation.cancel();
    }
    let wait = async {
      loop {
        let changed = self.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if self.active.lock().await.is_empty() {
          return;
        }
        changed.await;
      }
    };
    if tokio::time::timeout(std::time::Duration::from_secs(16), wait)
      .await
      .is_err()
    {
      tracing::warn!("worker shutdown deadline expired; reconcile attempts on restart");
    }
  }
}

impl TaskWorker {
  pub fn control(&self) -> WorkerControl {
    WorkerControl {
      active: self.active.clone(),
      changed: self.changed.clone(),
    }
  }

  pub async fn active_count(&self) -> usize {
    self.control().active_count().await
  }

  pub async fn shutdown(&self) {
    self.control().shutdown().await;
  }

  pub fn new(
    manager: PeerManager,
    executor: Arc<dyn WorkerExecutor>,
    workspace_root: PathBuf,
  ) -> Result<Self> {
    std::fs::create_dir_all(&workspace_root)?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(&workspace_root, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(Self {
      manager,
      executor,
      workspace_root: workspace_root.canonicalize()?,
      active: Arc::default(),
      submission: Arc::default(),
      changed: Arc::default(),
    })
  }

  /// Call before exposing the listener after a process restart. Never restart old work.
  pub fn recover_uncertain(&self) -> Result<usize> {
    let mut count = 0;
    for task in self.manager.store.tasks()? {
      if matches!(task.attempt_status.as_str(), "accepted" | "running") {
        self.manager.store.mark_task_uncertain(
          &task.envelope.task_id,
          &task.envelope.attempt_id,
          task.envelope.owner_epoch,
        )?;
        count += 1;
      }
    }
    Ok(count)
  }

  pub(crate) fn validate(&self, envelope: &TaskEnvelope) -> Result<PathBuf> {
    uuid::Uuid::parse_str(&envelope.task_id)?;
    uuid::Uuid::parse_str(&envelope.attempt_id)?;
    ensure!(
      !envelope.idempotency_key.is_empty() && envelope.idempotency_key.len() <= 256,
      "invalid idempotency key"
    );
    ensure!(
      !envelope.instructions.is_empty() && envelope.instructions.len() <= 131_072,
      "task instructions exceed limits"
    );
    let remaining = envelope.deadline_ms.saturating_sub(agent_core::now_ms());
    ensure!(
      remaining > 0 && remaining <= 86_400_000,
      "task deadline must be within the next 24 hours"
    );
    ensure!(
      envelope.capabilities.len() <= 128
        && envelope.capabilities.iter().all(|required| self
          .manager
          .node
          .capabilities
          .contains(required)),
      "worker lacks requested task capabilities"
    );
    Ok(
      self
        .workspace_root
        .join(&envelope.task_id)
        .join(&envelope.attempt_id),
    )
  }

  pub(crate) fn owned(&self, id: &str, owner: &str) -> Result<Task, tonic::Status> {
    let task = self
      .manager
      .store
      .task(id)
      .map_err(|_| tonic::Status::internal("task store unavailable"))?
      .ok_or_else(|| tonic::Status::not_found("task not found"))?;
    if task.envelope.owner_node_id != owner {
      return Err(tonic::Status::permission_denied(
        "task belongs to another master",
      ));
    }
    Ok(task)
  }
}

use std::{net::SocketAddr, time::Duration};

use agent_core::{Session, Task, TaskEnvelope};
use agent_distributed::{dispatcher::Dispatcher, peers::PeerManager};
use agent_test_support::{MockModelServer, StandardProtocol};
use agentd::{
  Daemon,
  config::Config,
  peers::{PeerRuntime, manager},
};
use clap::Parser;
use tempfile::TempDir;
use tokio::task::JoinHandle;

pub struct Pair {
  pub master: Daemon,
  pub worker: Daemon,
  pub master_manager: PeerManager,
  pub worker_manager: PeerManager,
  pub session: Session,
  pub dispatch: Dispatcher,
  master_data: TempDir,
  _worker_data: TempDir,
  servers: Vec<JoinHandle<anyhow::Result<()>>>,
}

impl Pair {
  pub async fn open(mock: &MockModelServer, protocol: StandardProtocol) -> Self {
    Self::open_with_models(mock, protocol, None).await
  }

  pub async fn open_with_models(
    mock: &MockModelServer,
    protocol: StandardProtocol,
    worker_models: Option<std::sync::Arc<agent_inference::ModelRegistry>>,
  ) -> Self {
    Self::configured(mock, mock, protocol, worker_models, |_, _| {}).await
  }

  #[allow(
    dead_code,
    reason = "shared fixture is compiled by independent integration test binaries"
  )]
  pub async fn open_configured(
    master_mock: &MockModelServer,
    worker_mock: &MockModelServer,
    protocol: StandardProtocol,
    configure: impl FnOnce(&mut Config, &Daemon),
  ) -> Self {
    Self::configured(master_mock, worker_mock, protocol, None, configure).await
  }

  async fn configured(
    master_mock: &MockModelServer,
    worker_mock: &MockModelServer,
    protocol: StandardProtocol,
    worker_models: Option<std::sync::Arc<agent_inference::ModelRegistry>>,
    configure: impl FnOnce(&mut Config, &Daemon),
  ) -> Self {
    let master_data = tempfile::tempdir().unwrap();
    let worker_data = tempfile::tempdir().unwrap();
    let worker = node(&worker_data, worker_mock, protocol, worker_models, |_| {}).await;
    let master = node(&master_data, master_mock, protocol, None, |config| {
      configure(config, &worker)
    })
    .await;
    let master_manager = manager(&master).unwrap();
    let worker_manager = manager(&worker).unwrap();
    let capabilities = [
      "tasks.submit",
      "tasks.read",
      "tasks.cancel",
      "tasks.accept",
      "workspace.read",
      "workspace.write.approval",
    ]
    .map(str::to_owned)
    .to_vec();
    master_manager
      .pair_local(worker_manager.public_identity(), capabilities.clone())
      .await
      .unwrap();
    worker_manager
      .pair_local(master_manager.public_identity(), capabilities)
      .await
      .unwrap();
    let workspace = master_data.path().join("master-workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("master-only.txt"), "master-private-content").unwrap();
    let session = master
      .store
      .create_session("master", workspace.to_str().unwrap(), &master.node_id)
      .await
      .unwrap();
    let dispatch = Dispatcher::new(master_manager.clone());
    Self {
      master,
      worker,
      master_manager,
      worker_manager,
      session,
      dispatch,
      master_data,
      _worker_data: worker_data,
      servers: Vec::new(),
    }
  }

  pub async fn serve(&mut self) {
    for daemon in [&self.master, &self.worker] {
      daemon.recover().await.unwrap();
      let runtime = PeerRuntime::prepare(daemon.clone()).await.unwrap().unwrap();
      self.servers.push(tokio::spawn(runtime.serve()));
    }
    tokio::time::timeout(Duration::from_secs(5), async {
      loop {
        if self
          .master_manager
          .connect(&self.worker_manager.local_node())
          .await
          .is_ok()
          && self
            .worker_manager
            .connect(&self.master_manager.local_node())
            .await
            .is_ok()
        {
          return;
        }
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
  }

  pub async fn envelope(&self, capabilities: Vec<String>) -> TaskEnvelope {
    let parent = self
      .master
      .store
      .runs(&self.session.id)
      .await
      .unwrap()
      .into_iter()
      .find(|run| matches!(run.status.as_str(), "accepted" | "running"));
    let parent = match parent {
      Some(parent) => parent,
      None => {
        self
          .master
          .store
          .accept_run(
            &self.session.id,
            self.session.owner_epoch,
            "coordinate Worker",
            &agent_core::new_id(),
          )
          .await
          .unwrap()
          .0
      }
    };
    if parent.status == "accepted" {
      self
        .master
        .store
        .update_run_owned(
          &parent.id,
          self.session.owner_epoch,
          agent_core::RunStatus::Running,
          "",
          "",
        )
        .await
        .unwrap();
    }
    TaskEnvelope {
      task_id: agent_core::new_id(),
      parent_run_id: parent.id,
      session_id: self.session.id.clone(),
      owner_node_id: self.master.node_id.clone(),
      owner_epoch: self.session.owner_epoch,
      attempt_id: agent_core::new_id(),
      instructions: "Read master-only.txt in your workspace, then summarize.".into(),
      capabilities,
      workspace: self
        .master_data
        .path()
        .join("master-workspace")
        .to_str()
        .unwrap()
        .into(),
      idempotency_key: agent_core::new_id(),
      deadline_ms: agent_core::now_ms() + 30_000,
    }
  }

  pub async fn terminal(&self, task_id: &str) -> Task {
    tokio::time::timeout(Duration::from_secs(10), async {
      loop {
        let task = self
          .dispatch
          .reconcile(&self.worker_manager.local_node(), task_id)
          .await
          .unwrap();
        if matches!(
          task.attempt_status.as_str(),
          "succeeded" | "failed" | "cancelled" | "uncertain"
        ) {
          return task;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap()
  }

  /// Keep the original five-second request deadline while surfacing terminal worker failures.
  pub async fn wait_for_worker_model(&self, mock: &MockModelServer, envelope: &TaskEnvelope) {
    let wait = async {
      let request = mock.wait_for_requests(1);
      tokio::pin!(request);
      loop {
        tokio::select! {
          biased;
          _ = &mut request => return,
          _ = tokio::time::sleep(Duration::from_millis(500)) => {
            let (finished, state) = self.worker_diagnostic(envelope).await;
            if finished {
              if !mock.requests().await.is_empty() { return; }
              panic!("worker ended before model request: {state}");
            }
          }
        }
      }
    };
    if tokio::time::timeout(Duration::from_secs(5), wait)
      .await
      .is_err()
    {
      let state = tokio::time::timeout(Duration::from_secs(2), self.worker_diagnostic(envelope))
        .await
        .map(|(_, state)| state)
        .unwrap_or_else(|_| "state unavailable".into());
      panic!("worker model request deadline elapsed: {state}");
    }
  }

  async fn worker_diagnostic(&self, envelope: &TaskEnvelope) -> (bool, String) {
    let Ok(task) = self.worker.store.task(&envelope.task_id).await else {
      return (false, "state unavailable".into());
    };
    let session = format!("worker:{}:{}", envelope.task_id, envelope.attempt_id);
    let Ok(runs) = self.worker.store.runs(&session).await else {
      return (false, "state unavailable".into());
    };
    let run = runs.last();
    let finished = task
      .as_ref()
      .is_some_and(|task| terminal_status(&task.attempt_status))
      || run.is_some_and(|run| terminal_status(&run.status));
    let state = format!(
      "task={}, attempt={}, run={}, error_class={}",
      safe_status(task.as_ref().map(|task| task.status.as_str())),
      safe_status(task.as_ref().map(|task| task.attempt_status.as_str())),
      safe_status(run.map(|run| run.status.as_str())),
      safe_error_class(
        run
          .map(|run| run.error.as_str())
          .filter(|error| !error.is_empty())
          .or_else(|| task.as_ref().map(|task| task.error.as_str()))
      ),
    );
    (finished, state)
  }

  pub async fn worker_runs(&self, envelope: &TaskEnvelope) -> Vec<agent_core::Run> {
    self
      .worker
      .store
      .runs(&format!(
        "worker:{}:{}",
        envelope.task_id, envelope.attempt_id
      ))
      .await
      .unwrap()
  }

  pub async fn stop(mut self) {
    self.master.shutdown().await;
    self.worker.shutdown().await;
    for server in self.servers.drain(..) {
      tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    }
  }

  pub async fn abort_worker_listener(&mut self) {
    let server = self.servers.pop().unwrap();
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
  }
}

async fn node(
  directory: &TempDir,
  mock: &MockModelServer,
  protocol: StandardProtocol,
  models: Option<std::sync::Arc<agent_inference::ModelRegistry>>,
  configure: impl FnOnce(&mut Config),
) -> Daemon {
  let reserve = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address: SocketAddr = reserve.local_addr().unwrap();
  let mut config = Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(directory.path().into());
  config.provider = protocol.provider().into();
  config.model = "worker-fixture".into();
  config.provider_api_key = Some("synthetic-worker-secret".into());
  config.api_base = Some(if protocol == StandardProtocol::Anthropic {
    mock.base_url().into()
  } else {
    format!("{}/v1", mock.base_url())
  });
  config.peer = Some(agent_config::PeerSettings {
    listen: address,
    advertise_url: format!("https://{address}"),
    mdns: false,
  });
  configure(&mut config);
  let daemon = if let Some(models) = models {
    config.provider = "local".into();
    config.provider_api_key = None;
    Daemon::open_with_models(config, models).await.unwrap()
  } else {
    Daemon::open(config).await.unwrap()
  };
  drop(reserve);
  daemon
}

fn terminal_status(status: &str) -> bool {
  matches!(
    status,
    "completed" | "succeeded" | "failed" | "cancelled" | "uncertain"
  )
}
fn safe_status(status: Option<&str>) -> &'static str {
  match status {
    None => "absent",
    Some("accepted") => "accepted",
    Some("running") => "running",
    Some("awaiting_approval") => "awaiting_approval",
    Some("awaiting_acceptance") => "awaiting_acceptance",
    Some("completed") => "completed",
    Some("succeeded") => "succeeded",
    Some("failed") => "failed",
    Some("cancelled") => "cancelled",
    Some("uncertain") => "uncertain",
    _ => "other",
  }
}
fn safe_error_class(error: Option<&str>) -> &'static str {
  let Some(error) = error.filter(|value| !value.is_empty()) else {
    return "none";
  };
  let error = error.to_ascii_lowercase();
  if error.contains("privacy") || error.contains("private") {
    "privacy"
  } else if error.contains("gateway") {
    "gateway"
  } else if error.contains("model") {
    "model"
  } else if error.contains("decision") || error.contains("classifier") {
    "decision"
  } else if error.contains("database") || error.contains("storage") {
    "storage"
  } else {
    "other"
  }
}

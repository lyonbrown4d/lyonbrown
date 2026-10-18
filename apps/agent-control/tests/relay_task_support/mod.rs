mod control;
mod logging;
pub use control::Control;

use agent_core::{Session, Task, TaskEnvelope};
use agent_distributed::{dispatcher::Dispatcher, peers::PeerManager};
use agent_protocol::v1;
use agent_test_support::{MockModelServer, StandardProtocol};
use agentd::{
  Daemon,
  config::Config,
  peers::{PeerRuntime, manager},
};
use clap::Parser;
use std::time::Duration;
use tokio::task::JoinHandle;

pub struct RelayPair {
  pub master: Daemon,
  pub worker: Daemon,
  pub session: Session,
  pub dispatch: Dispatcher,
  pub control: Control,
  master_manager: PeerManager,
  worker_manager: PeerManager,
  direct_master: PeerManager,
  direct_worker: PeerManager,
  _master_data: tempfile::TempDir,
  _worker_data: tempfile::TempDir,
  servers: Vec<JoinHandle<anyhow::Result<()>>>,
}
impl RelayPair {
  pub async fn start(model: &MockModelServer, protocol: StandardProtocol) -> Self {
    logging::init();
    let master_data = tempfile::tempdir().unwrap();
    let worker_data = tempfile::tempdir().unwrap();
    let mut master = node(&master_data, model, protocol).await;
    let mut worker = node(&worker_data, model, protocol).await;
    let direct_master = manager(&master).unwrap();
    let direct_worker = manager(&worker).unwrap();
    let mut control = Control::new().await;
    let task_caps = [
      "tasks.submit",
      "tasks.read",
      "tasks.cancel",
      "tasks.accept",
      "workspace.read",
    ]
    .map(String::from)
    .to_vec();
    direct_master
      .pair_local(direct_worker.public_identity(), task_caps.clone())
      .unwrap();
    direct_worker
      .pair_local(direct_master.public_identity(), task_caps)
      .unwrap();
    for peer in [&direct_master, &direct_worker] {
      peer
        .pair_local(
          control.identity.public.clone(),
          vec!["control.relay".into()],
        )
        .unwrap();
      control
        .manager
        .pair_local(
          peer.public_identity(),
          ["control.register", "control.resolve", "control.relay"]
            .map(String::from)
            .to_vec(),
        )
        .unwrap();
    }
    control
      .store
      .control_allow_link(&master.node_id, &worker.node_id, true)
      .unwrap();
    control
      .store
      .control_allow_link(&worker.node_id, &master.node_id, true)
      .unwrap();
    let master_id = master.node_id.clone();
    let worker_id = worker.node_id.clone();
    for (daemon, remote) in [(&mut master, worker_id), (&mut worker, master_id)] {
      daemon.config.relay = Some(agent_config::RelaySettings {
        control_node_id: control.identity.public.node_id.clone(),
        endpoint: control.endpoint.clone(),
        routes: [remote].into_iter().collect(),
      });
    }
    control.reconnect().await;
    let mut servers = Vec::new();
    for daemon in [&master, &worker] {
      daemon.recover().await.unwrap();
      let runtime = PeerRuntime::prepare(daemon.clone()).await.unwrap().unwrap();
      servers.push(tokio::spawn(runtime.serve()));
    }
    // Runtime installation publishes the selected relay handle. Earlier managers
    // intentionally remain direct-only for the negative route assertion.
    let master_manager = manager(&master).unwrap();
    let worker_manager = manager(&worker).unwrap();
    let workspace = master_data.path().join("master-workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(
      workspace.join("master-only.txt"),
      "master-private-relay-content",
    )
    .unwrap();
    let session = master
      .store
      .create_session("relay master", workspace.to_str().unwrap(), &master.node_id)
      .unwrap();
    let dispatch = Dispatcher::new(master_manager.clone());
    let pair = Self {
      master,
      worker,
      session,
      dispatch,
      control,
      master_manager,
      worker_manager,
      direct_master,
      direct_worker,
      _master_data: master_data,
      _worker_data: worker_data,
      servers,
    };
    pair.wait_connected().await;
    pair
  }
  pub fn worker_node(&self) -> v1::Node {
    self.worker_manager.local_node()
  }
  pub async fn wait_connected(&self) {
    tokio::time::timeout(Duration::from_secs(20), async {
      loop {
        if self
          .master_manager
          .connect(&self.worker_node())
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
        tokio::time::sleep(Duration::from_millis(100)).await;
      }
    })
    .await
    .expect("both outbound relay generations and inner mutual TLS connected");
  }
  pub async fn assert_direct_unreachable(&self) {
    assert!(
      self
        .direct_master
        .connect(&self.worker_node())
        .await
        .is_err()
    );
    assert!(
      self
        .direct_worker
        .connect(&self.master_manager.local_node())
        .await
        .is_err()
    );
    assert_eq!(
      self.worker_node().endpoint.split(':').nth(1),
      Some("//127.0.0.2")
    );
  }
  pub fn envelope(&self) -> TaskEnvelope {
    let (run, _) = self
      .master
      .store
      .accept_run(
        &self.session.id,
        self.session.owner_epoch,
        "coordinate relay worker",
        &agent_core::new_id(),
      )
      .unwrap();
    self
      .master
      .store
      .update_run_owned(
        &run.id,
        self.session.owner_epoch,
        agent_core::RunStatus::Running,
        "",
        "",
      )
      .unwrap();
    TaskEnvelope {
      task_id: agent_core::new_id(),
      parent_run_id: run.id,
      session_id: self.session.id.clone(),
      owner_node_id: self.master.node_id.clone(),
      owner_epoch: self.session.owner_epoch,
      attempt_id: agent_core::new_id(),
      instructions:
        "Private relay task: read master-only.txt in your isolated workspace, then summarize."
          .into(),
      capabilities: vec!["workspace.read".into()],
      workspace: self.session.workspace.clone(),
      idempotency_key: agent_core::new_id(),
      deadline_ms: agent_core::now_ms() + 90_000,
    }
  }
  pub async fn terminal(&self, task: &str) -> Task {
    tokio::time::timeout(Duration::from_secs(15), async {
      loop {
        let result = self
          .dispatch
          .reconcile(&self.worker_node(), task)
          .await
          .unwrap();
        if matches!(
          result.attempt_status.as_str(),
          "succeeded" | "failed" | "cancelled" | "uncertain"
        ) {
          return result;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    })
    .await
    .expect("reconciled worker terminal task")
  }
  pub async fn stop(&mut self) {
    self.master.shutdown().await;
    self.worker.shutdown().await;
    for server in self.servers.drain(..) {
      tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    }
    self.control.disconnect().await;
  }
}
pub fn worker_session(task: &TaskEnvelope) -> String {
  format!("worker:{}:{}", task.task_id, task.attempt_id)
}
async fn node(
  directory: &tempfile::TempDir,
  model: &MockModelServer,
  protocol: StandardProtocol,
) -> Daemon {
  let reserve = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = reserve.local_addr().unwrap();
  let mut config = Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(directory.path().into());
  config.provider = protocol.provider().into();
  config.model = "relay-worker-fixture".into();
  config.provider_api_key = Some("synthetic-relay-model-secret".into());
  config.api_base = Some(model.base_url().into());
  config.run_timeout_secs = 90;
  config.peer = Some(agent_config::PeerSettings {
    listen: address,
    advertise_url: format!("https://127.0.0.2:{}", address.port()),
    mdns: false,
  });
  let daemon = Daemon::open(config).await.unwrap();
  drop(reserve);
  daemon
}

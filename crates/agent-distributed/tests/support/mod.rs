use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};

use agent_distributed::{
  identity::PeerIdentity,
  peers::PeerManager,
  worker::{TaskWorker, WorkerExecutor, WorkerJob},
};
use agent_protocol::v1::{
  self, peer_service_server::PeerServiceServer, task_service_server::TaskServiceServer,
};
use agent_storage::Store;
use anyhow::Result;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;

pub struct Executor {
  pub calls: AtomicUsize,
  pub wait_for_cancel: bool,
}

#[async_trait::async_trait]
impl WorkerExecutor for Executor {
  async fn execute(&self, job: WorkerJob, cancellation: CancellationToken) -> Result<String> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    assert!(job.workspace.is_dir());
    assert_eq!(job.session.workspace, job.workspace.to_string_lossy());
    assert_ne!(job.session.id, job.task.session_id);
    assert_ne!(job.workspace.to_string_lossy(), job.task.workspace);
    if self.wait_for_cancel {
      cancellation.cancelled().await;
      anyhow::bail!("cancelled")
    }
    Ok("verified worker output".into())
  }
}

pub struct Node {
  pub directory: tempfile::TempDir,
  pub identity: PeerIdentity,
  pub manager: PeerManager,
  pub worker: TaskWorker,
  pub executor: Arc<Executor>,
  pub listener: Option<tokio::net::TcpListener>,
}

impl Node {
  pub async fn new(wait_for_cancel: bool) -> Result<Self> {
    let directory = tempfile::tempdir()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let identity = PeerIdentity::generate(&uuid::Uuid::now_v7().to_string())?;
    let store = Store::open(directory.path().join("state.db"))?;
    let node = v1::Node {
      id: identity.public.node_id.clone(),
      endpoint: format!("https://{}", listener.local_addr()?),
      protocol_version: 1,
      protocol_min: 1,
      protocol_max: 1,
      daemon_version: "0.1.0".into(),
      capabilities: vec!["test.execute".into()],
      ..Default::default()
    };
    let manager = PeerManager::new(store, identity.clone(), node)?;
    let executor = Arc::new(Executor {
      calls: AtomicUsize::new(0),
      wait_for_cancel,
    });
    let worker = TaskWorker::new(
      manager.clone(),
      executor.clone(),
      directory.path().join("workspaces"),
    )?;
    Ok(Self {
      directory,
      identity,
      manager,
      worker,
      executor,
      listener: Some(listener),
    })
  }
  pub fn store(&self) -> Result<Store> {
    Store::open(self.directory.path().join("state.db"))
  }
  pub fn pair(&self, other: &Self) -> Result<()> {
    self.manager.pair_local(
      other.identity.public.clone(),
      [
        "tasks.submit",
        "tasks.read",
        "tasks.cancel",
        "tasks.accept",
        "test.execute",
      ]
      .map(String::from)
      .to_vec(),
    )?;
    Ok(())
  }
  pub fn serve(
    &mut self,
  ) -> Result<(
    CancellationToken,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
  )> {
    let listener = self.listener.take().expect("listener served once");
    let manager = self.manager.clone();
    let worker = self.worker.clone();
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let mut server = tonic::transport::Server::builder().tls_config(manager.server_tls()?)?;
    let task = tokio::spawn(async move {
      server
        .add_service(PeerServiceServer::new(manager))
        .add_service(TaskServiceServer::new(worker))
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), stop.cancelled_owned())
        .await
    });
    Ok((shutdown, task))
  }
  pub fn envelope(&self) -> v1::TaskEnvelope {
    let session = self
      .store()
      .expect("test store")
      .create_session(
        "Master",
        "/master/absolute/path/must/not/be/reused",
        &self.identity.public.node_id,
      )
      .expect("test master session");
    let parent = self
      .store()
      .expect("test store")
      .accept_run(
        &session.id,
        1,
        "Master task orchestration",
        &uuid::Uuid::now_v7().to_string(),
      )
      .expect("test parent Run")
      .0;
    self
      .store()
      .expect("test store")
      .update_run_owned(&parent.id, 1, agent_core::RunStatus::Running, "", "")
      .expect("active parent Run");
    v1::TaskEnvelope {
      task_id: uuid::Uuid::now_v7().to_string(),
      attempt_id: uuid::Uuid::now_v7().to_string(),
      parent_run_id: parent.id,
      session_id: session.id,
      owner_node_id: self.identity.public.node_id.clone(),
      owner_epoch: 1,
      instructions: "synthetic authorized work".into(),
      capabilities: vec!["test.execute".into()],
      workspace: "/master/absolute/path/must/not/be/reused".into(),
      idempotency_key: uuid::Uuid::now_v7().to_string(),
      deadline_ms: agent_core::now_ms() + 10_000,
    }
  }
  pub async fn client(
    &self,
    remote: &Self,
  ) -> Result<agent_protocol::v1::task_service_client::TaskServiceClient<tonic::transport::Channel>>
  {
    let channel = tonic::transport::Endpoint::from_shared(remote.manager.local_node().endpoint)?
      .connect_timeout(Duration::from_secs(3))
      .timeout(Duration::from_secs(3))
      .tls_config(self.identity.client_tls(&remote.identity.public)?)?
      .connect()
      .await?;
    Ok(agent_protocol::v1::task_service_client::TaskServiceClient::new(channel))
  }
}

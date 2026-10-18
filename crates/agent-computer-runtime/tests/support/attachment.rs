use std::sync::{Arc, Mutex};

use agent_client::AgentClient;
use agent_protocol::v1::{
  self,
  client_service_server::{ClientService, ClientServiceServer},
  computer_runtime_service_server::{ComputerRuntimeService, ComputerRuntimeServiceServer},
};
use tokio::{net::TcpListener, sync::mpsc, task::JoinHandle};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{Request, Response, Status};
use uuid::Uuid;

type Requests = mpsc::Receiver<Result<v1::ComputerRuntimeRequest, Status>>;
#[derive(Clone)]
struct Service {
  session: v1::Session,
  requests: Arc<Mutex<Option<Requests>>>,
  responses: mpsc::Sender<v1::ComputerRuntimeResponse>,
}

pub struct Server {
  pub client: AgentClient,
  pub session: v1::Session,
  pub requests: Option<mpsc::Sender<Result<v1::ComputerRuntimeRequest, Status>>>,
  pub responses: mpsc::Receiver<v1::ComputerRuntimeResponse>,
  task: JoinHandle<()>,
}
impl Server {
  pub async fn start() -> Self {
    let session = v1::Session {
      id: Uuid::new_v4().to_string(),
      owner_node_id: Uuid::new_v4().to_string(),
      owner_epoch: 1,
      ..Default::default()
    };
    let (requests, request_receiver) = mpsc::channel(8);
    let (responses, response_receiver) = mpsc::channel(8);
    let service = Service {
      session: session.clone(),
      requests: Arc::new(Mutex::new(Some(request_receiver))),
      responses,
    };
    let listener = TcpListener::bind("127.0.0.1:0")
      .await
      .expect("fixture listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("fixture address"));
    let clients = ClientServiceServer::with_interceptor(service.clone(), authenticate);
    let computers = ComputerRuntimeServiceServer::with_interceptor(service, authenticate);
    let task = tokio::spawn(async move {
      tonic::transport::Server::builder()
        .add_service(clients)
        .add_service(computers)
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .expect("fixture server");
    });
    let client = AgentClient::connect(&endpoint, "synthetic-test-token")
      .await
      .expect("authenticated fixture client");
    Self {
      client,
      session,
      requests: Some(requests),
      responses: response_receiver,
      task,
    }
  }
  pub async fn send(&self, request: v1::ComputerRuntimeRequest) {
    self
      .requests
      .as_ref()
      .expect("request stream")
      .send(Ok(request))
      .await
      .expect("send fixture command");
  }
  pub async fn response(&mut self) -> v1::ComputerRuntimeResponse {
    tokio::time::timeout(std::time::Duration::from_secs(3), self.responses.recv())
      .await
      .expect("response deadline")
      .expect("response")
  }
}
impl Drop for Server {
  fn drop(&mut self) {
    self.task.abort();
  }
}
fn authenticate(request: Request<()>) -> Result<Request<()>, Status> {
  if request
    .metadata()
    .get("authorization")
    .and_then(|v| v.to_str().ok())
    == Some("Bearer synthetic-test-token")
  {
    Ok(request)
  } else {
    Err(Status::unauthenticated("fixture authentication required"))
  }
}
#[tonic::async_trait]
impl ComputerRuntimeService for Service {
  type AttachStream = ReceiverStream<Result<v1::ComputerRuntimeRequest, Status>>;
  async fn attach(
    &self,
    request: Request<tonic::Streaming<v1::ComputerRuntimeResponse>>,
  ) -> Result<Response<Self::AttachStream>, Status> {
    let requests = self
      .requests
      .lock()
      .expect("fixture requests")
      .take()
      .ok_or_else(|| Status::already_exists("fixture attachment"))?;
    let mut stream = request.into_inner();
    let responses = self.responses.clone();
    tokio::spawn(async move {
      while let Ok(Some(response)) = stream.message().await {
        if responses.send(response).await.is_err() {
          break;
        }
      }
    });
    Ok(Response::new(ReceiverStream::new(requests)))
  }
}
#[tonic::async_trait]
impl ClientService for Service {
  async fn get_node(&self, _: Request<v1::Empty>) -> Result<Response<v1::Node>, Status> {
    Ok(Response::new(v1::Node {
      id: self.session.owner_node_id.clone(),
      protocol_min: 1,
      protocol_max: 1,
      ..Default::default()
    }))
  }
  async fn list_sessions(&self, _: Request<v1::Empty>) -> Result<Response<v1::Sessions>, Status> {
    Ok(Response::new(v1::Sessions {
      sessions: vec![self.session.clone()],
    }))
  }
  async fn prepare_upgrade(&self, _: Request<v1::Empty>) -> Result<Response<v1::Node>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  async fn resume_accepting(&self, _: Request<v1::Empty>) -> Result<Response<v1::Node>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  async fn create_session(
    &self,
    _: Request<v1::CreateSessionRequest>,
  ) -> Result<Response<v1::Session>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  async fn get_session(
    &self,
    _: Request<v1::SessionRequest>,
  ) -> Result<Response<v1::Session>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  async fn start_run(&self, _: Request<v1::StartRunRequest>) -> Result<Response<v1::Run>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  async fn list_runs(&self, _: Request<v1::SessionRequest>) -> Result<Response<v1::Runs>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  async fn get_run(&self, _: Request<v1::RunRequest>) -> Result<Response<v1::Run>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  async fn cancel_run(&self, _: Request<v1::RunRequest>) -> Result<Response<v1::Run>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  async fn list_approvals(
    &self,
    _: Request<v1::RunRequest>,
  ) -> Result<Response<v1::Approvals>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  async fn approve_action(
    &self,
    _: Request<v1::ApproveRequest>,
  ) -> Result<Response<v1::Approval>, Status> {
    Err(Status::unimplemented("fixture"))
  }
  type WatchRunStream = ReceiverStream<Result<v1::Event, Status>>;
  async fn watch_run(
    &self,
    _: Request<v1::WatchRequest>,
  ) -> Result<Response<Self::WatchRunStream>, Status> {
    Err(Status::unimplemented("fixture"))
  }
}

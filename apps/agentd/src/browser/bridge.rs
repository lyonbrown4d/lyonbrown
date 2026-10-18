use crate::Daemon;
use agent_browser::{
  BrowserError, BrowserResult, Command, RUNTIME_PROTOCOL_VERSION, Reply, Space, Tab, wire,
};
use agent_protocol::v1::{
  self,
  browser_runtime_service_server::{BrowserRuntimeService, BrowserRuntimeServiceServer},
};
use agent_security::Authenticate;
use std::{collections::HashMap, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc, oneshot};
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};

struct Pending {
  result: oneshot::Sender<BrowserResult<Reply>>,
  read_only: bool,
}
struct Host {
  outgoing: mpsc::Sender<Result<v1::BrowserRuntimeRequest, Status>>,
  pending: HashMap<String, Pending>,
}
pub(crate) struct SpaceBinding {
  pub space: Space,
  pub host: String,
}
#[derive(Default)]
pub(crate) struct Routes {
  pub spaces: HashMap<String, SpaceBinding>,
  pub tabs: HashMap<String, Tab>,
}
#[derive(Default)]
pub(crate) struct BrowserRouter {
  hosts: Mutex<HashMap<String, Host>>,
  pub routes: Mutex<Routes>,
}
impl BrowserRouter {
  pub(crate) async fn has_runtime(&self) -> bool {
    !self.hosts.lock().await.is_empty()
  }
  pub async fn host(&self, requested: &str) -> Result<String, Status> {
    let hosts = self.hosts.lock().await;
    if !requested.is_empty() {
      return hosts
        .contains_key(requested)
        .then(|| requested.to_owned())
        .ok_or_else(|| Status::unavailable("browser runtime disconnected"));
    }
    if hosts.len() != 1 {
      return Err(Status::failed_precondition(
        "select one connected browser runtime",
      ));
    }
    hosts
      .keys()
      .next()
      .cloned()
      .ok_or_else(|| Status::unavailable("no browser runtime connected"))
  }
  pub async fn request(self: &Arc<Self>, host: &str, command: Command) -> BrowserResult<Reply> {
    let read_only = command.read_only();
    let id = agent_core::new_id();
    let payload = wire::encode(&command)?;
    let (send, receive) = oneshot::channel();
    let outgoing = {
      let mut hosts = self.hosts.lock().await;
      let entry = hosts
        .get_mut(host)
        .ok_or_else(|| BrowserError::Unavailable("runtime disconnected".into()))?;
      if entry.pending.len() >= 32 {
        return Err(BrowserError::Unavailable("runtime queue is full".into()));
      }
      entry.pending.insert(
        id.clone(),
        Pending {
          result: send,
          read_only,
        },
      );
      entry.outgoing.clone()
    };
    let mut guard = PendingGuard {
      router: self.clone(),
      host: host.into(),
      finished: false,
    };
    let response = tokio::time::timeout(Duration::from_secs(40), async {
      outgoing
        .send(Ok(v1::BrowserRuntimeRequest {
          request_id: id,
          payload,
        }))
        .await
        .map_err(|_| ())?;
      receive.await.map_err(|_| ())
    })
    .await;
    match response {
      Ok(Ok(reply)) => {
        guard.finished = true;
        reply
      }
      _ => {
        self.disconnect(host).await;
        Err(disconnected(read_only))
      }
    }
  }
  async fn disconnect(&self, host: &str) {
    if let Some(entry) = self.hosts.lock().await.remove(host) {
      let _ = entry
        .outgoing
        .try_send(Err(Status::unavailable("browser runtime lease ended")));
      for pending in entry.pending.into_values() {
        let _ = pending.result.send(Err(disconnected(pending.read_only)));
      }
    }
    let mut routes = self.routes.lock().await;
    routes.spaces.retain(|_, binding| binding.host != host);
    let spaces = routes
      .spaces
      .keys()
      .cloned()
      .collect::<std::collections::HashSet<_>>();
    routes.tabs.retain(|_, tab| spaces.contains(&tab.space_id));
  }
  async fn receive(&self, host: &str, response: v1::BrowserRuntimeResponse) -> BrowserResult<()> {
    if response.host_id != host || response.protocol_version != RUNTIME_PROTOCOL_VERSION {
      return Err(BrowserError::Protocol("runtime lease mismatch".into()));
    }
    let result: BrowserResult<Reply> = wire::decode(&response.payload)?;
    let pending = self
      .hosts
      .lock()
      .await
      .get_mut(host)
      .and_then(|entry| entry.pending.remove(&response.request_id))
      .ok_or_else(|| BrowserError::Protocol("unsolicited runtime response".into()))?;
    let _ = pending.result.send(result);
    Ok(())
  }
}
struct PendingGuard {
  router: Arc<BrowserRouter>,
  host: String,
  finished: bool,
}
impl Drop for PendingGuard {
  fn drop(&mut self) {
    if !self.finished
      && let Ok(handle) = tokio::runtime::Handle::try_current()
    {
      let router = self.router.clone();
      let host = self.host.clone();
      handle.spawn(async move {
        router.disconnect(&host).await;
      });
    }
  }
}
fn disconnected(read_only: bool) -> BrowserError {
  if read_only {
    BrowserError::Unavailable("runtime disconnected; observe again after reconnect".into())
  } else {
    BrowserError::Uncertain
  }
}
pub fn runtime_service(
  daemon: Daemon,
) -> InterceptedService<BrowserRuntimeServiceServer<Daemon>, Authenticate> {
  let auth = Authenticate::new(daemon.auth_token());
  InterceptedService::new(
    BrowserRuntimeServiceServer::new(daemon)
      .max_decoding_message_size(wire::MAX_PACKET_BYTES)
      .max_encoding_message_size(wire::MAX_PACKET_BYTES),
    auth,
  )
}
#[tonic::async_trait]
impl BrowserRuntimeService for Daemon {
  type AttachStream =
    Pin<Box<dyn futures::Stream<Item = Result<v1::BrowserRuntimeRequest, Status>> + Send>>;
  async fn attach(
    &self,
    request: Request<tonic::Streaming<v1::BrowserRuntimeResponse>>,
  ) -> Result<Response<Self::AttachStream>, Status> {
    let mut input = request.into_inner();
    let hello = tokio::time::timeout(Duration::from_secs(3), input.message())
      .await
      .map_err(|_| Status::deadline_exceeded("runtime hello deadline"))??
      .ok_or_else(|| Status::invalid_argument("runtime hello missing"))?;
    if uuid::Uuid::parse_str(&hello.host_id).is_err()
      || hello.protocol_version != RUNTIME_PROTOCOL_VERSION
      || !hello.payload.is_empty()
      || !hello.request_id.is_empty()
    {
      return Err(Status::invalid_argument("invalid runtime hello"));
    }
    let host = hello.host_id;
    let (outgoing, receiver) = mpsc::channel(8);
    let router = self.browser.clone();
    {
      let mut hosts = router.hosts.lock().await;
      if hosts.len() >= 8 || hosts.contains_key(&host) {
        return Err(Status::resource_exhausted(
          "runtime lease already exists or limit reached",
        ));
      }
      hosts.insert(
        host.clone(),
        Host {
          outgoing,
          pending: HashMap::new(),
        },
      );
    }
    let stop = self.stopping.clone();
    tokio::spawn(async move {
      loop {
        let response =
          tokio::select! {biased;_=stop.cancelled()=>break,response=input.message()=>response};
        match response {
          Ok(Some(response)) => {
            if router.receive(&host, response).await.is_err() {
              break;
            }
          }
          _ => break,
        }
      }
      router.disconnect(&host).await;
    });
    Ok(Response::new(Box::pin(tokio_stream_for(receiver))))
  }
}
fn tokio_stream_for(
  mut receiver: mpsc::Receiver<Result<v1::BrowserRuntimeRequest, Status>>,
) -> impl futures::Stream<Item = Result<v1::BrowserRuntimeRequest, Status>> + Send {
  async_stream::stream! {while let Some(request)=receiver.recv().await {yield request;}}
}

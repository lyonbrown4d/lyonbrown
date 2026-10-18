use super::{ComputerRuntimeInfo, Daemon};
use agent_computer_runtime::{RUNTIME_PROTOCOL_VERSION, RuntimeHello, wire};
use agent_protocol::{
  COMPUTER_RUNTIME_MESSAGE_LIMIT,
  v1::{
    self,
    computer_runtime_service_server::{ComputerRuntimeService, ComputerRuntimeServiceServer},
  },
};
use agent_security::Authenticate;
use std::{pin::Pin, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};
use uuid::Uuid;

pub fn computer_runtime_service(
  daemon: Daemon,
) -> InterceptedService<ComputerRuntimeServiceServer<Daemon>, Authenticate> {
  let auth = Authenticate::new(daemon.auth_token());
  InterceptedService::new(
    ComputerRuntimeServiceServer::new(daemon)
      .max_decoding_message_size(COMPUTER_RUNTIME_MESSAGE_LIMIT)
      .max_encoding_message_size(COMPUTER_RUNTIME_MESSAGE_LIMIT),
    auth,
  )
}

#[tonic::async_trait]
impl ComputerRuntimeService for Daemon {
  type AttachStream =
    Pin<Box<dyn futures::Stream<Item = Result<v1::ComputerRuntimeRequest, Status>> + Send>>;
  async fn attach(
    &self,
    request: Request<tonic::Streaming<v1::ComputerRuntimeResponse>>,
  ) -> Result<Response<Self::AttachStream>, Status> {
    let mut input = request.into_inner();
    let hello = tokio::time::timeout(Duration::from_secs(3), input.message())
      .await
      .map_err(|_| Status::deadline_exceeded("native runtime hello deadline"))??
      .ok_or_else(|| Status::invalid_argument("native runtime hello missing"))?;
    if !canonical_uuid(&hello.host_id)
      || !canonical_uuid(&hello.daemon_session_id)
      || !hello.request_id.is_empty()
      || hello.protocol_version != RUNTIME_PROTOCOL_VERSION
      || hello.payload.len() > 4096
    {
      return Err(Status::invalid_argument("invalid native runtime hello"));
    }
    let description: RuntimeHello = wire::decode(&hello.payload)
      .map_err(|_| Status::invalid_argument("invalid native runtime scope"))?;
    let native = &description.desktop_session;
    if native.id.is_nil()
      || native.identity.node_id != self.node_id
      || native.identity.user_session.is_empty()
      || native.identity.user_session.len() > 1024
    {
      return Err(Status::permission_denied(
        "native runtime identity mismatch",
      ));
    }
    let _submission = self.submission.lock().await;
    if self.draining.load(std::sync::atomic::Ordering::Acquire) {
      return Err(Status::unavailable("daemon is draining"));
    }
    let epoch = self.computer_session_epoch(&hello.daemon_session_id)?;
    let session = hello.daemon_session_id;
    let (outgoing, mut receiver) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let generation = self
      .computer
      .register(
        ComputerRuntimeInfo {
          host_id: hello.host_id,
          daemon_session_id: session.clone(),
          owner_epoch: epoch,
          desktop_session: description.desktop_session,
        },
        outgoing,
        stop.clone(),
      )
      .await?;
    let router = self.computer.clone();
    let daemon_stop = self.stopping.clone();
    let stream_stop = stop.clone();
    let stream_guard = StopOnDrop(stop.clone());
    tokio::spawn(async move {
      loop {
        let response = tokio::select! { biased;
          _ = daemon_stop.cancelled() => break,
          _ = stop.cancelled() => break,
          response = input.message() => response,
        };
        match response {
          Ok(Some(response)) => {
            if router
              .receive(&session, generation, response)
              .await
              .is_err()
            {
              break;
            }
          }
          _ => break,
        }
      }
      router.disconnect(&session, generation).await;
    });
    Ok(Response::new(Box::pin(async_stream::stream! {
      let _guard = stream_guard;
      loop {
        let request = tokio::select! { biased;
          _ = stream_stop.cancelled() => break,
          request = receiver.recv() => request,
        };
        match request { Some(request) => yield request, None => break }
      }
    })))
  }
}

fn canonical_uuid(value: &str) -> bool {
  Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

struct StopOnDrop(CancellationToken);
impl Drop for StopOnDrop {
  fn drop(&mut self) {
    self.0.cancel();
  }
}

use std::pin::Pin;

use agent_app::{AgentCommand, ModelSelection as AppModelSelection};
use agent_core::{MAX_MODEL_ID_BYTES, MAX_MODEL_PROVIDER_BYTES, ToolCallId};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::agent_service_server::AgentService;
use agent_protocol::v1::{
  CancelModelDownloadRequest, CancelTurnRequest, CommandResponse, ForceStopRequest,
  GetSnapshotRequest, HandshakeRequest, HandshakeResponse, HealthRequest, HealthResponse,
  HealthStatus, RespondApprovalRequest, RuntimeEvent, RuntimeSnapshot, SelectSessionModelRequest,
  StartModelDownloadRequest, StartTurnRequest, SteerCurrentTurnRequest, SubscribeEventsRequest,
};
use futures::{Stream, stream};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::AgentGrpcService;
use super::validation::{approval_decision, validate_metadata, validate_required_bounded};
use crate::mapping::{event_to_wire, snapshot_to_wire};
use crate::replay::fingerprint;

const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_CLIENT_NAME_BYTES: usize = 128;
const MAX_CLIENT_VERSION_BYTES: usize = 64;
const MAX_MODEL_DOWNLOAD_ID_BYTES: usize = 128;
type EventStream = Pin<Box<dyn Stream<Item = Result<RuntimeEvent, Status>> + Send>>;

#[tonic::async_trait]
impl AgentService for AgentGrpcService {
  type SubscribeEventsStream = EventStream;

  async fn health(
    &self,
    request: Request<HealthRequest>,
  ) -> Result<Response<HealthResponse>, Status> {
    validate_metadata(request.into_inner().metadata)?;
    Ok(Response::new(HealthResponse {
      protocol_version: PROTOCOL_VERSION,
      status: HealthStatus::Serving as i32,
    }))
  }

  async fn handshake(
    &self,
    request: Request<HandshakeRequest>,
  ) -> Result<Response<HandshakeResponse>, Status> {
    let request = request.into_inner();
    validate_metadata(request.metadata)?;
    validate_required_bounded("client_name", &request.client_name, MAX_CLIENT_NAME_BYTES)?;
    validate_required_bounded(
      "client_version",
      &request.client_version,
      MAX_CLIENT_VERSION_BYTES,
    )?;
    let mut capabilities = vec!["streaming".into(), "approvals".into()];
    capabilities.extend(self.host.capabilities().iter().map(|value| (*value).into()));
    Ok(Response::new(HandshakeResponse {
      protocol_version: PROTOCOL_VERSION,
      server_version: env!("CARGO_PKG_VERSION").into(),
      server_instance_id: self.server_instance_id.clone(),
      capabilities,
      scope_fingerprint: self.scope_fingerprint.as_str().to_owned(),
    }))
  }

  async fn start_turn(
    &self,
    request: Request<StartTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    let request = request.into_inner();
    validate_required_bounded("text", &request.text, MAX_TEXT_BYTES)?;
    let command_fingerprint = fingerprint("start_turn", &[request.text.as_bytes()]);
    self
      .execute(
        request.metadata,
        AgentCommand::StartTurn { text: request.text },
        command_fingerprint,
      )
      .await
  }

  async fn steer_current_turn(
    &self,
    request: Request<SteerCurrentTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    let request = request.into_inner();
    validate_required_bounded("text", &request.text, MAX_TEXT_BYTES)?;
    let command_fingerprint = fingerprint("steer_current_turn", &[request.text.as_bytes()]);
    self
      .execute(
        request.metadata,
        AgentCommand::SteerCurrentTurn { text: request.text },
        command_fingerprint,
      )
      .await
  }

  async fn queue_follow_up(
    &self,
    request: Request<agent_protocol::v1::QueueFollowUpRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    let request = request.into_inner();
    validate_required_bounded("text", &request.text, MAX_TEXT_BYTES)?;
    let command_fingerprint = fingerprint("queue_follow_up", &[request.text.as_bytes()]);
    self
      .execute(
        request.metadata,
        AgentCommand::QueueFollowUp { text: request.text },
        command_fingerprint,
      )
      .await
  }

  async fn cancel_turn(
    &self,
    request: Request<CancelTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self
      .execute(
        request.into_inner().metadata,
        AgentCommand::CancelTurn,
        fingerprint("cancel_turn", &[]),
      )
      .await
  }

  async fn force_stop(
    &self,
    request: Request<ForceStopRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self
      .execute(
        request.into_inner().metadata,
        AgentCommand::ForceStop,
        fingerprint("force_stop", &[]),
      )
      .await
  }

  async fn respond_approval(
    &self,
    request: Request<RespondApprovalRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    let request = request.into_inner();
    validate_required_bounded("request_id", &request.request_id, 64)?;
    let request_id = Uuid::parse_str(&request.request_id)
      .map(ToolCallId)
      .map_err(|_| Status::invalid_argument("request_id must be a UUID"))?;
    let decision = approval_decision(request.decision)?;
    let decision_bytes = request.decision.to_be_bytes();
    let command_fingerprint = fingerprint(
      "respond_approval",
      &[request.request_id.as_bytes(), &decision_bytes],
    );
    self
      .execute(
        request.metadata,
        AgentCommand::RespondApproval {
          request_id,
          decision,
        },
        command_fingerprint,
      )
      .await
  }

  async fn select_session_model(
    &self,
    request: Request<SelectSessionModelRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    let request = request.into_inner();
    let selection = request
      .selection
      .ok_or_else(|| Status::invalid_argument("selection is required"))?;
    let (selection, command_fingerprint) = match selection.selection {
      Some(agent_protocol::v1::model_selection::Selection::Auto(true)) => (
        AppModelSelection::Auto,
        fingerprint("select_session_model", &[b"auto"]),
      ),
      Some(agent_protocol::v1::model_selection::Selection::Pinned(route)) => {
        validate_required_bounded("provider", &route.provider, MAX_MODEL_PROVIDER_BYTES)?;
        validate_required_bounded("model", &route.model, MAX_MODEL_ID_BYTES)?;
        let command_fingerprint = fingerprint(
          "select_session_model",
          &[b"pinned", route.provider.as_bytes(), route.model.as_bytes()],
        );
        (
          AppModelSelection::Pinned {
            provider: route.provider,
            model: route.model,
          },
          command_fingerprint,
        )
      }
      _ => return Err(Status::invalid_argument("invalid model selection")),
    };
    self
      .execute(
        request.metadata,
        AgentCommand::SelectSessionModel { selection },
        command_fingerprint,
      )
      .await
  }

  async fn start_model_download(
    &self,
    request: Request<StartModelDownloadRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    let request = request.into_inner();
    validate_required_bounded("model_id", &request.model_id, MAX_MODEL_DOWNLOAD_ID_BYTES)?;
    let command_fingerprint = fingerprint("start_model_download", &[request.model_id.as_bytes()]);
    self
      .execute(
        request.metadata,
        AgentCommand::StartModelDownload {
          model_id: request.model_id,
        },
        command_fingerprint,
      )
      .await
  }

  async fn cancel_model_download(
    &self,
    request: Request<CancelModelDownloadRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    let request = request.into_inner();
    validate_required_bounded(
      "operation_id",
      &request.operation_id,
      MAX_MODEL_DOWNLOAD_ID_BYTES,
    )?;
    let command_fingerprint =
      fingerprint("cancel_model_download", &[request.operation_id.as_bytes()]);
    self
      .execute(
        request.metadata,
        AgentCommand::CancelModelDownload {
          operation_id: request.operation_id,
        },
        command_fingerprint,
      )
      .await
  }

  async fn get_snapshot(
    &self,
    request: Request<GetSnapshotRequest>,
  ) -> Result<Response<RuntimeSnapshot>, Status> {
    validate_metadata(request.into_inner().metadata)?;
    Ok(Response::new(snapshot_to_wire(self.host.snapshot())))
  }

  async fn subscribe_events(
    &self,
    request: Request<SubscribeEventsRequest>,
  ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
    let request = request.into_inner();
    validate_metadata(request.metadata)?;
    let receiver = self.host.subscribe();
    let after_revision = request.after_revision;
    let shutdown = self.stream_shutdown.clone();
    let events = stream::unfold(
      (receiver, true, shutdown),
      move |(mut receiver, mut first, shutdown)| async move {
        loop {
          if shutdown.is_cancelled() {
            return None;
          }
          let snapshot = if first {
            first = false;
            receiver.borrow().clone()
          } else {
            tokio::select! {
              _ = shutdown.cancelled() => return None,
              changed = receiver.changed() => changed.ok()?,
            }
            receiver.borrow_and_update().clone()
          };
          if snapshot.revision > after_revision {
            return Some((Ok(event_to_wire(snapshot)), (receiver, first, shutdown)));
          }
        }
      },
    );
    Ok(Response::new(Box::pin(events)))
  }
}

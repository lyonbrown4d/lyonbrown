use agent_app::ModelSelection;
use agent_protocol::v1::model_selection::Selection;
use agent_protocol::v1::{
  CancelTurnRequest, CommandResponse, ForceStopRequest, ModelRoute,
  ModelSelection as WireModelSelection, QueueFollowUpRequest, RespondApprovalRequest,
  SelectSessionModelRequest, StartTurnRequest, SteerCurrentTurnRequest,
};
use uuid::Uuid;

use crate::AgentClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
  AllowOnce,
  AllowForSession,
  DenyOnce,
  DenyRemaining,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
  StartTurn {
    text: String,
  },
  SteerCurrentTurn {
    text: String,
  },
  QueueFollowUp {
    text: String,
  },
  CancelTurn,
  ForceStop,
  RespondApproval {
    request_id: String,
    decision: ApprovalDecision,
  },
  SelectSessionModel {
    selection: ModelSelection,
  },
}

impl AgentClient {
  pub async fn execute_with_id(
    &mut self,
    command_id: impl Into<String>,
    command: Command,
  ) -> Result<CommandResponse, tonic::Status> {
    let metadata = Some(self.metadata_with_id(command_id.into()));
    self.execute_with_metadata(metadata, command).await
  }

  pub(crate) async fn execute_with_id_for_server(
    &mut self,
    command_id: String,
    server_instance_id: String,
    command: Command,
  ) -> Result<CommandResponse, tonic::Status> {
    let metadata = Some(self.metadata_with_id_for_server(command_id, server_instance_id));
    self.execute_with_metadata(metadata, command).await
  }

  async fn execute_with_metadata(
    &mut self,
    metadata: Option<agent_protocol::v1::RequestMetadata>,
    command: Command,
  ) -> Result<CommandResponse, tonic::Status> {
    let response = match command {
      Command::StartTurn { text } => {
        self
          .inner
          .start_turn(StartTurnRequest { metadata, text })
          .await?
      }
      Command::SteerCurrentTurn { text } => {
        self
          .inner
          .steer_current_turn(SteerCurrentTurnRequest { metadata, text })
          .await?
      }
      Command::QueueFollowUp { text } => {
        self
          .inner
          .queue_follow_up(QueueFollowUpRequest { metadata, text })
          .await?
      }
      Command::CancelTurn => {
        self
          .inner
          .cancel_turn(CancelTurnRequest { metadata })
          .await?
      }
      Command::ForceStop => self.inner.force_stop(ForceStopRequest { metadata }).await?,
      Command::RespondApproval {
        request_id,
        decision,
      } => {
        self
          .inner
          .respond_approval(RespondApprovalRequest {
            metadata,
            request_id,
            decision: wire_decision(decision) as i32,
          })
          .await?
      }
      Command::SelectSessionModel { selection } => {
        self
          .inner
          .select_session_model(SelectSessionModelRequest {
            metadata,
            selection: Some(selection_to_wire(selection)),
          })
          .await?
      }
    };
    Ok(response.into_inner())
  }

  pub async fn start_turn(
    &mut self,
    text: impl Into<String>,
  ) -> Result<CommandResponse, tonic::Status> {
    self
      .execute_new(Command::StartTurn { text: text.into() })
      .await
  }

  pub async fn steer_current_turn(
    &mut self,
    text: impl Into<String>,
  ) -> Result<CommandResponse, tonic::Status> {
    self
      .execute_new(Command::SteerCurrentTurn { text: text.into() })
      .await
  }

  pub async fn queue_follow_up(
    &mut self,
    text: impl Into<String>,
  ) -> Result<CommandResponse, tonic::Status> {
    self
      .execute_new(Command::QueueFollowUp { text: text.into() })
      .await
  }

  pub async fn cancel_turn(&mut self) -> Result<CommandResponse, tonic::Status> {
    self.execute_new(Command::CancelTurn).await
  }

  pub async fn force_stop(&mut self) -> Result<CommandResponse, tonic::Status> {
    self.execute_new(Command::ForceStop).await
  }

  pub async fn respond_approval(
    &mut self,
    request_id: impl Into<String>,
    decision: ApprovalDecision,
  ) -> Result<CommandResponse, tonic::Status> {
    self
      .execute_new(Command::RespondApproval {
        request_id: request_id.into(),
        decision,
      })
      .await
  }

  pub async fn select_session_model(
    &mut self,
    selection: ModelSelection,
  ) -> Result<CommandResponse, tonic::Status> {
    self
      .execute_new(Command::SelectSessionModel { selection })
      .await
  }

  async fn execute_new(&mut self, command: Command) -> Result<CommandResponse, tonic::Status> {
    self
      .execute_with_id(Uuid::new_v4().to_string(), command)
      .await
  }
}

fn selection_to_wire(selection: ModelSelection) -> WireModelSelection {
  let selection = match selection {
    ModelSelection::Auto => Selection::Auto(true),
    ModelSelection::Pinned { provider, model } => Selection::Pinned(ModelRoute { provider, model }),
  };
  WireModelSelection {
    selection: Some(selection),
  }
}

fn wire_decision(decision: ApprovalDecision) -> agent_protocol::v1::ApprovalDecision {
  use agent_protocol::v1::ApprovalDecision as WireDecision;
  match decision {
    ApprovalDecision::AllowOnce => WireDecision::AllowOnce,
    ApprovalDecision::AllowForSession => WireDecision::AllowForSession,
    ApprovalDecision::DenyOnce => WireDecision::DenyOnce,
    ApprovalDecision::DenyRemaining => WireDecision::DenyRemaining,
  }
}

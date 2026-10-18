use agent_computer::{
  DesktopActionReceipt, DesktopActionRequest, DesktopCapabilities, DesktopError, DesktopSession,
  DesktopSnapshot, LeaseToken, ObservationLimits,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub type RuntimeResult<T> = Result<T, RuntimeError>;

pub const RUNTIME_PROTOCOL_VERSION: u32 = 1;

/// Immutable native scope selected by the interactive user before attachment.
#[derive(Clone, Serialize, Deserialize)]
pub struct RuntimeHello {
  pub desktop_session: DesktopSession,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunBinding {
  pub daemon_session_id: String,
  pub run_id: String,
  pub owner_epoch: u64,
}

impl RunBinding {
  pub fn validate(&self) -> RuntimeResult<()> {
    if self.owner_epoch == 0
      || !Uuid::parse_str(&self.daemon_session_id).is_ok_and(|id| !id.is_nil())
      || !Uuid::parse_str(&self.run_id).is_ok_and(|id| !id.is_nil())
    {
      return Err(RuntimeError::InvalidBinding);
    }
    Ok(())
  }
}

/// Private authenticated host input. Never expose to models or web content.
/// `Execute` attests that the daemon has durably consumed the exact approval and
/// execution intent. Transport authentication and persistence belong to the host.
#[derive(Clone, Serialize, Deserialize)]
pub struct Command {
  pub binding: RunBinding,
  pub operation: Operation,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Operation {
  Capabilities,
  AcquireLease {
    ttl_ms: u64,
  },
  Observe {
    lease: LeaseToken,
    limits: ObservationLimits,
    timeout_ms: u64,
  },
  Prepare {
    request: DesktopActionRequest,
  },
  /// Uses only the immutable preparation retained in this process. A failed or
  /// interrupted execution consumes the handle too; automatic replay is forbidden.
  Execute {
    preparation_id: Uuid,
    action_id: Uuid,
    content_digest: [u8; 32],
    approval_id: Uuid,
    timeout_ms: u64,
  },
  Revoke {
    lease: LeaseToken,
  },
  Receipt {
    action_id: Uuid,
  },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Preparation {
  pub binding: RunBinding,
  pub preparation_id: Uuid,
  pub request: DesktopActionRequest,
  pub content_digest: [u8; 32],
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
  pub binding: RunBinding,
  pub preparation_id: Uuid,
  /// Snapshot identity approved originally; receipt identifies the refreshed one.
  pub original_request: DesktopActionRequest,
  pub content_digest: [u8; 32],
  pub receipt: DesktopActionReceipt,
}

/// Private text, pixels, accessibility labels and titles never implement Debug
/// through this model. Callers must not log serialized commands or replies.
#[derive(Clone, Serialize, Deserialize)]
pub enum Reply {
  Capabilities(DesktopCapabilities),
  Lease(LeaseToken),
  Snapshot(DesktopSnapshot),
  Prepared(Preparation),
  Executed(ExecutionResult),
  Revoked,
  Receipt(Option<ExecutionResult>),
}

#[derive(Clone, Debug, Eq, PartialEq, Error, Serialize, Deserialize)]
pub enum RuntimeError {
  #[error("invalid run binding")]
  InvalidBinding,
  #[error("run binding does not own this operation")]
  BindingMismatch,
  #[error("preparation is absent or consumed")]
  PreparationUnavailable,
  #[error("execution does not match the immutable preparation")]
  PreparationMismatch,
  #[error("invalid operation timeout or approval identity")]
  InvalidControl,
  #[error("bounded runtime identity capacity exceeded")]
  CapacityExceeded,
  #[error("private runtime protocol failure")]
  Protocol,
  #[error("private runtime packet exceeds 68 MiB")]
  PacketTooLarge,
  #[error("runtime coordination unavailable")]
  CoordinationUnavailable,
  #[error(transparent)]
  Desktop(#[from] DesktopError),
}

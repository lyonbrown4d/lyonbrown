//! Versioned wire contracts. No execution-engine or UI types cross this boundary.
pub mod v1 {
  tonic::include_proto!("agent.v1");
}

mod mapping;
/// 64 MiB RGBA plus bounded runtime metadata; protobuf envelope has extra room.
pub const COMPUTER_RUNTIME_PAYLOAD_LIMIT: usize = 68 * 1024 * 1024;
pub const COMPUTER_RUNTIME_MESSAGE_LIMIT: usize = COMPUTER_RUNTIME_PAYLOAD_LIMIT + 1024;

impl std::fmt::Debug for v1::RunImageAttachment {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RunImageAttachment")
      .field("mime_type", &self.mime_type)
      .field("bytes", &self.data.len())
      .field("sha256", &self.sha256)
      .finish()
  }
}
impl std::fmt::Debug for v1::StartRunWithAttachmentsRequest {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("StartRunWithAttachmentsRequest")
      .field("session_id", &self.session_id)
      .field("owner_epoch", &self.owner_epoch)
      .field("images", &self.images)
      .finish_non_exhaustive()
  }
}

//! Generated wire types for the Lyonbrown local agent protocol.

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_GRPC_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

pub mod v1 {
  tonic::include_proto!("lyonbrown.agent.v1");

  impl RuntimeSnapshot {
    pub fn phase_name(&self) -> &'static str {
      RuntimePhase::try_from(self.phase)
        .unwrap_or(RuntimePhase::Unspecified)
        .as_str_name()
        .strip_prefix("RUNTIME_PHASE_")
        .unwrap_or("UNSPECIFIED")
    }
  }
}

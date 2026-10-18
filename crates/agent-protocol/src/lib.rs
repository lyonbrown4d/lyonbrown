//! Versioned wire contracts. No execution-engine or UI types cross this boundary.
pub mod v1 {
  tonic::include_proto!("agent.v1");
}

mod mapping;
/// 64 MiB RGBA plus bounded runtime metadata; protobuf envelope has extra room.
pub const COMPUTER_RUNTIME_PAYLOAD_LIMIT: usize = 68 * 1024 * 1024;
pub const COMPUTER_RUNTIME_MESSAGE_LIMIT: usize = COMPUTER_RUNTIME_PAYLOAD_LIMIT + 1024;

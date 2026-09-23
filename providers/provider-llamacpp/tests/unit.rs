include!("../src/lib.rs");

#[cfg(not(feature = "llama-cpp"))]
#[path = "../src/helpers.rs"]
mod helpers;
#[cfg(not(feature = "llama-cpp"))]
#[path = "../src/tool_protocol.rs"]
mod tool_protocol;
#[cfg(not(feature = "llama-cpp"))]
use helpers::*;
#[cfg(not(feature = "llama-cpp"))]
use tool_protocol::*;

#[cfg(not(feature = "llama-cpp"))]
use agent_core::{Locality, ModelCapabilities, ModelTier, StopReason};
#[cfg(not(feature = "llama-cpp"))]
use agent_core::{ToolCallId, ToolDefinition, ToolInvocation};
#[cfg(not(feature = "llama-cpp"))]
use agent_model::{ModelEvent, ModelRequest, Role};
#[cfg(not(feature = "llama-cpp"))]
use std::{ops::Range, sync::Arc};
#[cfg(not(feature = "llama-cpp"))]
use tokio::sync::{Mutex, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore, mpsc};

mod tests {
  include!("unit/fixtures.rs");
  include!("unit/cases.rs");
  include!("unit/tool_protocol.rs");
}

use std::collections::BTreeSet;

use agent_core::TaskEnvelope;
use anyhow::{Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Arguments {
  pub call: Operation,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Operation {
  Dispatch {
    target_id: String,
    instructions: String,
    capabilities: BTreeSet<String>,
    timeout_ms: u64,
  },
  Get {
    target_id: String,
    task_id: String,
  },
  Reconcile {
    target_id: String,
    task_id: String,
  },
  Cancel {
    target_id: String,
    task_id: String,
  },
  Accept {
    target_id: String,
    task_id: String,
    accepted: bool,
  },
}

impl Operation {
  pub fn target(&self) -> &str {
    match self {
      Self::Dispatch { target_id, .. }
      | Self::Get { target_id, .. }
      | Self::Reconcile { target_id, .. }
      | Self::Cancel { target_id, .. }
      | Self::Accept { target_id, .. } => target_id,
    }
  }
}

pub(super) fn envelope(
  tool: &super::DelegationTool,
  call: &str,
  instructions: String,
  capabilities: BTreeSet<String>,
  timeout_ms: u64,
) -> Result<TaskEnvelope> {
  ensure!(
    !call.is_empty() && call.len() <= 4096,
    "invalid delegation call identifier"
  );
  ensure!(
    !instructions.trim().is_empty() && instructions.len() <= 32 * 1024,
    "delegation instructions must be nonempty and at most 32 KiB"
  );
  ensure!(
    (1..=tool
      .daemon
      .config
      .run_timeout_secs
      .saturating_mul(1000)
      .min(86_400_000))
      .contains(&timeout_ms),
    "delegation timeout exceeds the authorized run limit"
  );
  ensure!(
    capabilities.is_subset(&tool.capabilities),
    "delegation exceeds the execution plan scope"
  );
  let task_id = stable_id(&tool.run_id, call, "task");
  let attempt_id = stable_id(&tool.run_id, call, "attempt");
  Ok(TaskEnvelope {
    task_id,
    attempt_id,
    parent_run_id: tool.run_id.clone(),
    session_id: tool.session_id.clone(),
    owner_node_id: tool.daemon.node_id.clone(),
    owner_epoch: tool.epoch,
    instructions,
    capabilities: capabilities.into_iter().collect(),
    workspace: tool.workspace.clone(),
    idempotency_key: stable_id(&tool.run_id, call, "idempotency"),
    deadline_ms: agent_core::now_ms().saturating_add(timeout_ms as i64),
  })
}

fn stable_id(run: &str, call: &str, purpose: &str) -> String {
  let mut hash = Sha256::new();
  for value in [run, call, purpose] {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value.as_bytes());
  }
  let digest = hash.finalize();
  let mut bytes = [0; 16];
  bytes.copy_from_slice(&digest[..16]);
  // UUIDv8 carries this application-defined SHA-256 identity; separators cannot collide.
  uuid::Builder::from_custom_bytes(bytes)
    .into_uuid()
    .to_string()
}

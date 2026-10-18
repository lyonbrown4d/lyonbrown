use agent_runtime::ToolCall;
use agent_workspace::process::CommandRequest;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

pub(super) fn request(run: &str, call: &ToolCall) -> Result<CommandRequest> {
  ensure!(
    !call.id.is_empty() && call.id.len() <= 1024,
    "invalid command call identity"
  );
  let args = call
    .arguments
    .as_object()
    .ok_or_else(|| anyhow::anyhow!("command arguments must be an object"))?;
  ensure!(
    args.keys().all(|key| [
      "executable",
      "arguments",
      "cwd",
      "timeout_ms",
      "output_limit_bytes"
    ]
    .contains(&key.as_str())),
    "unknown command argument"
  );
  let text = |key: &str, default: Option<&str>| -> Result<String> {
    match args.get(key) {
      Some(value) => Ok(
        value
          .as_str()
          .ok_or_else(|| anyhow::anyhow!("invalid command {key}"))?
          .into(),
      ),
      None => default
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("command {key} is required")),
    }
  };
  let number = |key: &str, default: u64| -> Result<u64> {
    args.get(key).map_or(Ok(default), |value| {
      value
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("invalid command {key}"))
    })
  };
  let arguments = args.get("arguments").map_or(Ok(Vec::new()), |value| {
    value
      .as_array()
      .ok_or_else(|| anyhow::anyhow!("command arguments must be an array"))?
      .iter()
      .map(|value| {
        value
          .as_str()
          .map(str::to_owned)
          .ok_or_else(|| anyhow::anyhow!("command arguments must be strings"))
      })
      .collect::<Result<Vec<_>>>()
  })?;
  let mut hash = Sha256::new();
  hash.update(b"lyonbrown-command-v1\0");
  hash.update(run.as_bytes());
  hash.update([0]);
  hash.update(call.id.as_bytes());
  Ok(CommandRequest {
    action_id: format!("command-{}", hex::encode(hash.finalize())),
    executable: text("executable", None)?.into(),
    arguments,
    cwd: text("cwd", Some("."))?.into(),
    timeout_ms: number("timeout_ms", 30_000)?,
    output_limit_bytes: usize::try_from(number("output_limit_bytes", 65_536)?)?,
  })
}

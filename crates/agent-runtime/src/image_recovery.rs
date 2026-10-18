//! Capability gate until upstream raw JSON content roundtrips are fixed.
//! This inspects only known image positions; it does not normalize or restore
//! upstream messages through a JSON Value compatibility path.
//! Upstream: https://github.com/bigfish1913/pi-rust/issues/59. Remove this gate
//! only after adopting the fixed release and verifying actual image recovery.
use serde_json::Value;

use crate::{Result, RuntimeError};

fn message(value: &Value) -> bool {
  value["content"]
    .as_array()
    .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "image"))
}

fn messages(value: &Value) -> bool {
  value
    .as_array()
    .is_some_and(|values| values.iter().any(message))
}

fn entry(value: &Value) -> bool {
  match value["type"].as_str() {
    Some("message") => message(&value["message"]),
    Some("compaction") => messages(&value["retainedTail"]),
    _ => false,
  }
}

fn entries(value: &Value) -> bool {
  value
    .as_array()
    .is_some_and(|values| values.iter().any(entry))
}

fn record(value: &Value) -> bool {
  match value["type"].as_str() {
    Some("operation_started") if value["intent"]["kind"] == "run" => {
      messages(&value["intent"]["originalPrompt"]) || entries(&value["intent"]["initialMessages"])
    }
    Some("queue_enqueued" | "write_deferred") => entry(&value["target"]),
    Some("assistant_frame") if value["op"] == "append" && value["frame"]["type"] == "start" => {
      message(&value["frame"]["partial"])
    }
    _ => false,
  }
}

pub(super) fn check(bytes: &[u8]) -> Result<()> {
  let snapshot: Value =
    serde_json::from_slice(bytes).map_err(|error| RuntimeError::Persistence(error.to_string()))?;
  let has_images = entries(&snapshot["state"]["entries"])
    || snapshot["state"]["records"]
      .as_array()
      .is_some_and(|records| records.iter().any(record));
  if has_images {
    return Err(RuntimeError::Configuration(
      "image-bearing session recovery is unavailable until upstream rpi-ai snapshot serialization is fixed".into(),
    ));
  }
  Ok(())
}

#[cfg(test)]
#[path = "../tests/support/image_recovery.rs"]
mod tests;

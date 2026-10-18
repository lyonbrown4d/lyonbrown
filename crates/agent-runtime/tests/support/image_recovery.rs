use super::*;
use serde_json::json;

#[test]
fn known_image_locations_fail_closed_without_mutating_or_scanning_business_data() {
  let message = json!({"kind":"user", "content":[{"type":"image", "data":"synthetic"}]});
  let entry = json!({"type":"message", "message":message});
  let compact = json!({"type":"compaction", "retainedTail":[message.clone()]});
  let snapshots = [
    json!({"state":{"entries":[entry.clone()]}}),
    json!({"state":{"entries":[compact]}}),
    json!({"state":{"records":[{"type":"operation_started", "intent":{"kind":"run","originalPrompt":[message.clone()]}}]}}),
    json!({"state":{"records":[{"type":"operation_started", "intent":{"kind":"run","initialMessages":[entry.clone()]}}]}}),
    json!({"state":{"records":[{"type":"queue_enqueued", "target":entry.clone()}]}}),
    json!({"state":{"records":[{"type":"write_deferred", "target":entry}]}}),
    json!({"state":{"records":[{"type":"assistant_frame", "op":"append", "frame":{"type":"start","partial":message}}]}}),
  ];
  for snapshot in snapshots {
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    assert!(
      check(&bytes)
        .unwrap_err()
        .to_string()
        .contains("image-bearing session recovery is unavailable")
    );
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), snapshot);
  }
  let business = json!({"state":{"entries":[{"type":"custom","data":{"type":"image","data":"business-value"}}]}});
  check(&serde_json::to_vec(&business).unwrap()).unwrap();
  assert!(matches!(
    check(b"not-json"),
    Err(RuntimeError::Persistence(_))
  ));
}

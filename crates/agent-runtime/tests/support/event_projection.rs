use super::*;
use rpi_agent::message::CustomMessage;

const PIXELS: &str = "synthetic-image-pixels";

fn image() -> Content {
  Content::Image(ImageContent {
    kind: ImageContentType,
    data: PIXELS.into(),
    mime_type: "image/png".into(),
  })
}

fn projected_image() -> Value {
  json!({"type":"image", "mimeType":"image/png", "dataRedacted":true, "encodedBytes":PIXELS.len()})
}

fn flat_entry(message: Value) -> Value {
  json!({"type":"message", "id":"entry", "seq":1, "parentId":null, "timestamp":3, "message":message})
}

fn user() -> AgentMessage {
  AgentMessage::User(UserMessage::new(
    UserContent::Blocks(vec![Content::text("before"), image()]),
    2,
  ))
}

fn assert_no_pixels(value: &Value) {
  assert!(!value.to_string().contains(PIXELS));
  assert!(!format!("{value:?}").contains(PIXELS));
}

#[test]
fn all_message_roles_project_only_typed_images_and_keep_business_data() {
  let mut assistant = AssistantMessage::empty(Api::OpenaiCompletions, "fixture", "model", 2);
  assistant.content = vec![
    Content::tool_call("call", "fixture", json!({"data":"business-value"})),
    image(),
  ];
  let tool: AgentMessage = serde_json::from_value(json!({
    "kind":"tool_result", "role":"toolResult", "toolCallId":"call", "toolName":"fixture",
    "content":[{"type":"text","text":"before"}, {"type":"image","data":PIXELS,"mimeType":"image/png"}],
    "details":{"data":"business-value"}, "isError":false, "timestamp":2,
  })).unwrap();
  let custom = AgentMessage::Custom(CustomMessage::new(
    "fixture",
    vec![Content::text("before"), image()],
    json!({"data":"business-value"}),
    2,
  ));
  for original in [
    user(),
    AgentMessage::Assistant(Box::new(assistant)),
    tool,
    custom,
  ] {
    let entry: Entry = serde_json::from_value(flat_entry(json!(original))).unwrap();
    let projected = super::entry(&entry);
    assert_no_pixels(&projected);
    assert_eq!(projected["id"], "entry");
    assert_eq!(projected["seq"], 1);
    assert_eq!(projected["message"]["content"][1], projected_image());
    match original {
      AgentMessage::Assistant(_) => assert_eq!(
        projected["message"]["content"][0]["arguments"]["data"],
        "business-value"
      ),
      AgentMessage::ToolResult(_) => {
        assert_eq!(projected["message"]["details"]["data"], "business-value")
      }
      AgentMessage::Custom(_) => assert_eq!(projected["message"]["data"]["data"], "business-value"),
      AgentMessage::User(_) => assert_eq!(projected["message"]["content"][0]["text"], "before"),
    }
  }
}

#[test]
fn compaction_operation_and_queued_targets_preserve_metadata_without_pixels() {
  let compact: Entry = serde_json::from_value(json!({
    "type":"compaction", "id":"compact", "seq":2,"parentId":"entry","timestamp":4,
    "summary":"retain context", "retainedTail":[user()], "tokensBefore":12, "details":{"data":"business-value"},
  })).unwrap();
  let projected = super::entry(&compact);
  assert_no_pixels(&projected);
  assert_eq!(
    projected["retainedTail"][0]["content"][1],
    projected_image()
  );
  assert_eq!(projected["details"]["data"], "business-value");
  let mut target = json!(compact);
  for key in ["seq", "parentId", "timestamp"] {
    target.as_object_mut().unwrap().remove(key);
  }
  let base = json!({"id":"record", "seq":3,"lane":"main", "timestamp":5});
  for (kind, fields) in [
    (
      "operation_started",
      json!({"sourceLeafId":"entry", "intent":{"kind":"run", "originalPrompt":[user()], "initialMessages":[target.clone()]}}),
    ),
    (
      "queue_enqueued",
      json!({"queue":"nextRun", "target":target.clone()}),
    ),
    ("write_deferred", json!({"runId":"run", "target":target})),
  ] {
    let mut record = base.clone();
    record["type"] = json!(kind);
    record
      .as_object_mut()
      .unwrap()
      .extend(fields.as_object().unwrap().clone());
    let record: LaneRecord = serde_json::from_value(record).unwrap();
    let projected = super::record(&record);
    assert_no_pixels(&projected);
    assert_eq!(projected["id"], "record");
    let target = if kind == "operation_started" {
      assert_eq!(
        projected["intent"]["originalPrompt"][0]["content"][1],
        projected_image()
      );
      &projected["intent"]["initialMessages"][0]
    } else {
      &projected["target"]
    };
    assert_eq!(target["retainedTail"][0]["content"][1], projected_image());
    assert_eq!(target["details"]["data"], "business-value");
    assert!(target.get("seq").is_none());
  }
}

#[test]
fn frames_and_unknown_schemas_do_not_forward_unchecked_image_payloads() {
  let mut partial = AssistantMessage::empty(Api::OpenaiCompletions, "fixture", "model", 2);
  partial.content.push(image());
  let projected = frame(&json!({"type":"start", "partial":partial}));
  assert_no_pixels(&projected);
  assert_eq!(projected["partial"]["content"][0], projected_image());
  assert_eq!(frame(&json!({"type":"unknown","data":PIXELS})), omitted());
  assert_eq!(
    provisioned(&json!({"type":"unknown","data":PIXELS})),
    omitted()
  );
  assert_eq!(
    provisioned(&json!({"type":"message","id":"entry","message":{"data":PIXELS}})),
    omitted()
  );
  let debug = format!(
    "{:?}",
    crate::RuntimeImage {
      mime_type: "image/png".into(),
      data_base64: PIXELS.into()
    }
  );
  assert!(!debug.contains(PIXELS));
  assert!(debug.contains("encoded_bytes"));
}

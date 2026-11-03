use agent_runtime::ToolDefinition;
use serde_json::json;
pub(super) fn definition(name: &str) -> ToolDefinition {
  let (description, properties, required) = match name {
    "patch_preview" => (
      "Preview a bounded multi-file unified patch without modifying files. Read file revisions first. Returned SHA identifies the exact reviewable checkpoint.",
      json!({"request_id":{"type":"string","format":"uuid"},"files":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"object","additionalProperties":false,"properties":{"path":{"type":"string"},"expected_revision":{"type":"string"},"patch":{"type":"string"},"delete":{"type":"boolean"}},"required":["path","expected_revision","patch"]}}}),
      vec!["request_id", "files"],
    ),
    "patch_apply" => (
      "Apply a stored preview only after exact human approval. Commits intent first. Partial effects become uncertain and must not be replayed. Filesystem writes are not a cross-file atomic transaction.",
      json!({"request_id":{"type":"string","format":"uuid"},"change_id":{"type":"string"},"expected_revision":{"type":"integer","minimum":1},"preview_sha256":{"type":"string"},"action_id":{"type":"string","format":"uuid"}}),
      vec![
        "request_id",
        "change_id",
        "expected_revision",
        "preview_sha256",
        "action_id",
      ],
    ),
    "checkpoint_undo" => (
      "Create a reviewable reversal preview for an applied checkpoint. Refuses if current files differ from its after-images. Apply the returned preview through patch_apply with fresh approval.",
      json!({"request_id":{"type":"string","format":"uuid"},"change_id":{"type":"string"}}),
      vec!["request_id", "change_id"],
    ),
    "validation_record" => (
      "Record validation evidence from an actual persisted execute_command operation after the applied change. Status is host-derived; model claims are not validation evidence.",
      json!({"request_id":{"type":"string","format":"uuid"},"change_id":{"type":"string"},"expected_revision":{"type":"integer","minimum":1},"operation_id":{"type":"string"}}),
      vec![
        "request_id",
        "change_id",
        "expected_revision",
        "operation_id",
      ],
    ),
    _ => (
      "Read a stored checkpoint and its actual host validation records in this session.",
      json!({"change_id":{"type":"string"}}),
      vec!["change_id"],
    ),
  };
  ToolDefinition {
    name: name.into(),
    description: description.into(),
    parameters: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
  }
}

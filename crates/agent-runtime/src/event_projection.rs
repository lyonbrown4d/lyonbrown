//! Public mutation payloads are separate from the lossless Harness snapshot.
//! Only typed content images are projected; arbitrary tool/custom data is not
//! recursively rewritten. Unknown provisioned/frame schemas omit their payload.
use rpi_agent::message::AgentMessage;
use rpi_ai::{frames::AssistantMessageFrame, types::*};
use rpi_harness::session::types::{AssistantFrameOp, Entry, LaneRecord, OperationIntent};
use serde_json::{Value, json};

fn content(blocks: &[Content]) -> Value {
  Value::Array(
    blocks
      .iter()
      .map(|block| match block {
        Content::Image(image) => json!({
          "type": "image", "mimeType": image.mime_type,
          "dataRedacted": true, "encodedBytes": image.data.len(),
        }),
        Content::Text(text) => json!(text),
        Content::Thinking(thinking) => json!(thinking),
        Content::ToolCall(call) => json!(call),
      })
      .collect(),
  )
}

fn assistant(message: &AssistantMessage) -> Value {
  json!({
    "kind": "assistant", "role": "assistant", "content": content(&message.content),
    "api": message.api, "provider": message.provider, "model": message.model,
    "responseModel": message.response_model, "responseId": message.response_id,
    "usage": message.usage, "stopReason": message.stop_reason,
    "deferred": message.deferred, "errorMessage": message.error_message,
    "rawStopReason": message.raw_stop_reason, "endTurn": message.end_turn,
    "timestamp": message.timestamp,
  })
}

fn message(message: &AgentMessage) -> Value {
  match message {
    AgentMessage::User(user) => {
      let projected = match &user.content {
        UserContent::Text(text) => json!(text),
        UserContent::Blocks(blocks) => content(blocks),
      };
      json!({"kind":"user", "role":"user", "content":projected, "timestamp":user.timestamp})
    }
    AgentMessage::Assistant(value) => assistant(value),
    AgentMessage::ToolResult(tool) => json!({
      "kind":"tool_result", "role":"toolResult", "content":content(&tool.content),
      "toolCallId":tool.tool_call_id, "toolName":tool.tool_name,
      "details":tool.details, "usage":tool.usage, "addedToolNames":tool.added_tool_names,
      "isError":tool.is_error, "timestamp":tool.timestamp,
    }),
    AgentMessage::Custom(custom) => json!({
      "kind":"custom", "role":custom.role, "content":content(&custom.content),
      "data":custom.data, "timestamp":custom.timestamp,
    }),
  }
}

pub(super) fn entry(value: &Entry) -> Value {
  let mut projected = json!(value.base());
  projected["type"] = json!(value.entry_type());
  match value {
    Entry::Message(value) => {
      projected["message"] = message(&value.message);
      projected["terminate"] = json!(value.terminate);
    }
    Entry::Compaction(value) => {
      projected["summary"] = json!(value.summary);
      projected["retainedTail"] = Value::Array(value.retained_tail.iter().map(message).collect());
      projected["tokensBefore"] = json!(value.tokens_before);
      projected["details"] = json!(value.details);
      projected["usage"] = json!(value.usage);
    }
    Entry::ModelChange(_)
    | Entry::ThinkingLevel(_)
    | Entry::ActiveTools(_)
    | Entry::BranchSummary(_)
    | Entry::Custom(_) => return json!(value),
  }
  projected
}

fn omitted() -> Value {
  json!({"projectionOmitted":true})
}

fn provisioned(value: &Value) -> Value {
  // Upstream stores a flat entry without storage fields in queue/intent targets.
  // Decode that exact schema so unknown nested content cannot pass through.
  let Some(mut fields) = value.as_object().cloned() else {
    return omitted();
  };
  fields.insert("seq".into(), json!(0));
  fields.insert("parentId".into(), Value::Null);
  fields.insert("timestamp".into(), json!(0));
  let Ok(typed) = serde_json::from_value::<Entry>(Value::Object(fields)) else {
    return omitted();
  };
  let mut projected = entry(&typed);
  if let Some(fields) = projected.as_object_mut() {
    fields.remove("seq");
    fields.remove("parentId");
    fields.remove("timestamp");
  }
  projected
}

fn intent(value: &OperationIntent) -> Value {
  match value {
    OperationIntent::Run {
      original_prompt,
      initial_messages,
      system_prompt_override,
      resume_data,
    } => json!({
      "kind":"run", "originalPrompt":original_prompt.iter().map(message).collect::<Vec<_>>(),
      "initialMessages":initial_messages.iter().map(provisioned).collect::<Vec<_>>(),
      "systemPromptOverride":system_prompt_override, "resumeData":resume_data,
    }),
    OperationIntent::Compaction { .. } | OperationIntent::Navigation { .. } => json!(value),
  }
}

fn frame(value: &Value) -> Value {
  let Ok(frame) = serde_json::from_value::<AssistantMessageFrame>(value.clone()) else {
    return omitted();
  };
  match &frame {
    AssistantMessageFrame::Start { partial } => {
      let mut partial = assistant(partial);
      if let Some(fields) = partial.as_object_mut() {
        fields.remove("kind");
      }
      json!({"type":"start", "partial":partial})
    }
    AssistantMessageFrame::TextStart { .. }
    | AssistantMessageFrame::TextDelta { .. }
    | AssistantMessageFrame::TextEnd { .. }
    | AssistantMessageFrame::ThinkingStart { .. }
    | AssistantMessageFrame::ThinkingDelta { .. }
    | AssistantMessageFrame::ThinkingEnd { .. }
    | AssistantMessageFrame::ToolcallStart { .. }
    | AssistantMessageFrame::ToolcallCheckpoint { .. }
    | AssistantMessageFrame::ToolcallDelta { .. }
    | AssistantMessageFrame::ToolcallEnd { .. } => json!(frame),
  }
}

pub(super) fn record(value: &LaneRecord) -> Value {
  let mut projected = json!(value.base());
  projected["type"] = json!(value.record_type());
  match value {
    LaneRecord::OperationStarted(value) => {
      projected["sourceLeafId"] = json!(value.source_leaf_id);
      projected["intent"] = intent(&value.intent);
    }
    LaneRecord::QueueEnqueued(value) => {
      projected["queue"] = json!(value.queue);
      projected["runId"] = json!(value.run_id);
      projected["target"] = provisioned(&value.target);
    }
    LaneRecord::WriteDeferred(value) => {
      projected["runId"] = json!(value.run_id);
      projected["target"] = provisioned(&value.target);
    }
    LaneRecord::AssistantFrame(value) => {
      projected["runId"] = json!(value.run_id);
      projected["streamIndex"] = json!(value.stream_index);
      projected["op"] = json!(value.op);
      projected["frame"] = match (&value.op, &value.frame) {
        (AssistantFrameOp::Append, Some(value)) => frame(value),
        (AssistantFrameOp::CommitStream, Some(value)) => {
          match value.get("entryId").and_then(Value::as_str) {
            Some(id) => json!({"entryId":id}),
            None => omitted(),
          }
        }
        (AssistantFrameOp::ClearStream | AssistantFrameOp::ClearRun, _) | (_, None) => Value::Null,
      };
    }
    LaneRecord::AbortRequested(_)
    | LaneRecord::OperationFinished(_)
    | LaneRecord::StepAttempt(_)
    | LaneRecord::ToolStarted(_)
    | LaneRecord::QueueCancelled(_)
    | LaneRecord::Usage(_)
    | LaneRecord::RetryPending(_) => return json!(value),
  }
  projected
}

#[cfg(test)]
#[path = "../tests/support/event_projection.rs"]
mod tests;

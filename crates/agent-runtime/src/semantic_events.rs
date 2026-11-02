//! Translate typed Harness persistence facts into application contracts here.
//! Streamed text/thinking are emitted by the live adapter exactly once.
use agent_core::events::{AgentEvent, ExecutionPhase, QueueKind, QueueStatus, TokenUsage};
use rpi_agent::message::AgentMessage;
use rpi_ai::{Content, Usage, UserContent};
use rpi_harness::session::types::{Entry, LaneRecord, QueueEnqueuedRecord};

fn queue_kind(kind: rpi_harness::session::types::QueueKind) -> Option<QueueKind> {
  match kind {
    rpi_harness::session::types::QueueKind::Steer => Some(QueueKind::Steer),
    rpi_harness::session::types::QueueKind::FollowUp => Some(QueueKind::FollowUp),
    _ => None,
  }
}

fn queued<'a>(id: &str, records: &'a [LaneRecord]) -> Option<&'a QueueEnqueuedRecord> {
  records.iter().rev().find_map(|record| match record {
    LaneRecord::QueueEnqueued(value) if value.target.get("id")?.as_str()? == id => Some(value),
    _ => None,
  })
}

fn text(content: &[Content]) -> String {
  content
    .iter()
    .filter_map(|value| match value {
      Content::Text(text) => Some(text.text.as_str()),
      _ => None,
    })
    .collect()
}

fn prompt(value: &serde_json::Value) -> Option<String> {
  // Decode the exact provisioned entry schema; never search arbitrary JSON
  // recursively for content or forward SDK objects to clients.
  let mut value = value.as_object()?.clone();
  value.insert("seq".into(), 0.into());
  value.insert("parentId".into(), serde_json::Value::Null);
  value.insert("timestamp".into(), 0.into());
  let Entry::Message(entry) = serde_json::from_value(serde_json::Value::Object(value)).ok()? else {
    return None;
  };
  match entry.message {
    AgentMessage::User(value) => Some(match value.content {
      UserContent::Text(value) => value,
      UserContent::Blocks(value) => text(&value),
    }),
    _ => None,
  }
}

fn usage(entry_id: &str, value: &Usage) -> AgentEvent {
  AgentEvent::UsageUpdated {
    entry_id: entry_id.into(),
    usage: TokenUsage {
      input: value.input.max(0) as u64,
      output: value.output.max(0) as u64,
      cache_read: value.cache_read.max(0) as u64,
      cache_write: value.cache_write.max(0) as u64,
      total: value.total_tokens.max(0) as u64,
    },
  }
}

pub(crate) fn entry(value: &Entry, records: &[LaneRecord]) -> Vec<AgentEvent> {
  let mut events = Vec::new();
  if let Some(queue) = queued(&value.base().id, records)
    && let Some(kind) = queue_kind(queue.queue)
  {
    events.push(AgentEvent::QueueUpdated {
      entry_id: value.base().id.clone(),
      kind,
      status: QueueStatus::Consumed,
      prompt: None,
      request_id: None,
    });
  }
  if let Entry::Message(value) = value
    && let AgentMessage::ToolResult(tool) = &value.message
  {
    events.push(AgentEvent::ToolFinished {
      call_id: tool.tool_call_id.clone(),
      name: tool.tool_name.clone(),
      text: text(&tool.content),
      is_error: tool.is_error,
    });
  }
  // Ordinary assistant turns report usage on the committed entry. A separate
  // Usage record is optional. Both sources use the same entry key, so later
  // records replace the reported counters instead of adding them a second time.
  let reported = match value {
    Entry::Message(value) => match &value.message {
      AgentMessage::Assistant(value) => Some(&value.usage),
      AgentMessage::ToolResult(value) => value.usage.as_ref(),
      _ => None,
    },
    Entry::Compaction(value) => value.usage.as_ref(),
    Entry::BranchSummary(value) => value.usage.as_ref(),
    _ => None,
  };
  if let Some(reported) = reported {
    events.push(usage(&value.base().id, reported));
  }
  events
}

pub(crate) fn record(value: &LaneRecord, records: &[LaneRecord]) -> Vec<AgentEvent> {
  match value {
    LaneRecord::Usage(value) => vec![usage(
      // The record id is stable even for hook/adjustment usage without an entry.
      value.entry_id.as_deref().unwrap_or(&value.base.id),
      &value.usage,
    )],
    LaneRecord::QueueEnqueued(value) => match (
      queue_kind(value.queue),
      value.target.get("id").and_then(serde_json::Value::as_str),
    ) {
      (Some(kind), Some(id)) => vec![AgentEvent::QueueUpdated {
        entry_id: id.into(),
        kind,
        status: QueueStatus::Pending,
        prompt: prompt(&value.target),
        request_id: None,
      }],
      _ => Vec::new(),
    },
    LaneRecord::QueueCancelled(value) => {
      match queued(&value.entry_id, records).and_then(|queue| queue_kind(queue.queue)) {
        Some(kind) => vec![AgentEvent::QueueUpdated {
          entry_id: value.entry_id.clone(),
          kind,
          status: QueueStatus::Cancelled,
          prompt: None,
          request_id: None,
        }],
        None => Vec::new(),
      }
    }
    LaneRecord::ToolStarted(value) => vec![
      AgentEvent::PhaseChanged {
        phase: ExecutionPhase::PreparingTool,
        model: None,
      },
      AgentEvent::ToolStarted {
        call_id: value.tool_call_id.clone(),
        name: value.tool_name.clone(),
        arguments: value.effective_args.clone(),
      },
    ],
    _ => Vec::new(),
  }
}

//! Readable protocol projections. Returned text never changes client authority.
use agent_protocol::v1;

pub(crate) fn state_name(state: v1::MemoryState) -> &'static str {
  match state {
    v1::MemoryState::Proposed => "proposed",
    v1::MemoryState::Active => "active",
    v1::MemoryState::Superseded => "superseded",
    v1::MemoryState::Rejected => "rejected",
    v1::MemoryState::Deleted => "deleted",
    _ => "unknown",
  }
}
fn state(value: i32) -> &'static str {
  v1::MemoryState::try_from(value)
    .map(state_name)
    .unwrap_or("unknown")
}
fn scope(value: &v1::MemoryRecord) -> &'static str {
  match value
    .scope
    .as_ref()
    .and_then(|scope| v1::MemoryScopeKind::try_from(scope.kind).ok())
  {
    Some(v1::MemoryScopeKind::User) => "user",
    Some(v1::MemoryScopeKind::Project) => "project",
    Some(v1::MemoryScopeKind::Session) => "session",
    _ => "unknown",
  }
}
fn kind(value: i32) -> &'static str {
  match v1::MemoryKind::try_from(value).ok() {
    Some(v1::MemoryKind::Preference) => "preference",
    Some(v1::MemoryKind::Fact) => "fact",
    Some(v1::MemoryKind::Procedure) => "procedure",
    _ => "unknown",
  }
}
fn text(value: &str) -> String {
  value
    .chars()
    .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
    .collect()
}
fn summary(value: &v1::MemoryRecord) -> String {
  let preview = text(&value.content)
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");
  let preview: String = preview.chars().take(120).collect();
  format!(
    "{} · rev {} · {} · {} · {}\n  {}",
    text(&value.id),
    value.revision,
    state(value.state),
    scope(value),
    kind(value.kind),
    preview
  )
}
pub(crate) fn record(value: &v1::MemoryRecord) -> String {
  format!(
    "ID: {}\nRevision: {}\nState: {}\nScope: {}\nKind: {}\n\n{}",
    text(&value.id),
    value.revision,
    state(value.state),
    scope(value),
    kind(value.kind),
    text(&value.content)
  )
}
pub(crate) fn page(value: &v1::Memories, filter: &str) -> String {
  let mut lines = value.items.iter().map(summary).collect::<Vec<_>>();
  if lines.is_empty() {
    lines.push("No matching memories".into());
  }
  if !value.next_cursor.is_empty() {
    lines.push(format!(
      "Next page: /memory list {filter} {}",
      text(&value.next_cursor)
    ));
  }
  lines.join("\n\n")
}
pub(crate) fn matches(value: &v1::MemoryMatches) -> String {
  let lines = value
    .items
    .iter()
    .filter_map(|value| {
      value
        .memory
        .as_ref()
        .map(|memory| format!("Score {:.3}\n{}", value.score, summary(memory)))
    })
    .collect::<Vec<_>>();
  if lines.is_empty() {
    "No matching active memories".into()
  } else {
    lines.join("\n\n")
  }
}
pub(crate) fn status(value: &v1::MemoryStatus) -> String {
  format!(
    "Retrieval: {}\nVector backend: {}\nEmbedding model: {}\nDimensions: {}\n{}",
    if value.retrieval_ready {
      "ready"
    } else {
      "unavailable"
    },
    text(&value.vector_backend),
    text(&value.embedding_model),
    value.dimensions,
    text(&value.detail)
  )
}
pub(crate) fn jobs(value: &v1::MemoryJobs) -> String {
  if value.jobs.is_empty() {
    return "No index jobs".into();
  }
  value
    .jobs
    .iter()
    .map(|job| {
      let state = match v1::MemoryJobState::try_from(job.state).ok() {
        Some(v1::MemoryJobState::Pending) => "pending",
        Some(v1::MemoryJobState::Running) => "running",
        Some(v1::MemoryJobState::Completed) => "completed",
        Some(v1::MemoryJobState::Failed) => "failed",
        Some(v1::MemoryJobState::Uncertain) => "uncertain",
        _ => "unknown",
      };
      let kind = match v1::MemoryJobKind::try_from(job.kind).ok() {
        Some(v1::MemoryJobKind::Upsert) => "upsert",
        Some(v1::MemoryJobKind::Delete) => "delete",
        _ => "unknown",
      };
      format!(
        "Job: {}\nMemory: {} · rev {} · {kind} · {state}\nAttempts: {} · error category: {}",
        text(&job.id),
        text(&job.memory_id),
        job.revision,
        job.attempts,
        job
          .error_category
          .as_deref()
          .map(text)
          .unwrap_or_else(|| "none".into())
      )
    })
    .collect::<Vec<_>>()
    .join("\n\n")
}

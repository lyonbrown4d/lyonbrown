use agent_app::{ActivityPhase, ActivityView, ToolActivityState};
use agent_core::StopReason;

use super::{
  ActivityItem, Locale, activity_json::bounded_json, locale::ActivityMessages, status_line_safe,
  terminal_safe,
};

const FIELD_LIMIT: usize = 1_024;
const FIELD_SCAN_LIMIT: usize = 2_048;
const MAX_FALLBACKS: usize = 8;
const MAX_TOOLS: usize = 16;
const MAX_TOOL_CALL_IDS: usize = 16;
const MAX_DETAIL_LINES: usize = 128;
const MAX_DETAIL_CHARS: usize = 16_000;

pub(crate) fn detail(item: &ActivityItem, locale: Locale) -> Vec<String> {
  let messages = locale.messages().inspector;
  let view = &item.view;
  let collections_truncated = view
    .route
    .as_ref()
    .is_some_and(|route| route.fallbacks.len() > MAX_FALLBACKS)
    || view.tools.len() > MAX_TOOLS
    || view
      .outcome
      .as_ref()
      .is_some_and(|outcome| outcome.tool_calls.len() > MAX_TOOL_CALL_IDS);
  let mut lines = Vec::new();
  append_route(&mut lines, view, messages);
  lines.push(format!(
    "{}: {}",
    messages.phase,
    phase_label(view.phase, messages)
  ));
  lines.push(format!("turn: {}", item.target.turn_id));
  lines.push(format!("step: {}", item.target.step_id));
  append_context_usage(&mut lines, view, messages);
  if !view.assistant_text.is_empty() {
    lines.push(format!(
      "{}: {}",
      messages.assistant_text,
      bounded(&view.assistant_text, messages)
    ));
  }
  append_tools(&mut lines, view, messages);
  append_outcome(&mut lines, view, messages);
  enforce_total_budget(lines, messages, collections_truncated)
}

fn append_context_usage(lines: &mut Vec<String>, view: &ActivityView, messages: ActivityMessages) {
  if let Some(context) = &view.context {
    lines.push(format!(
      "{}: {} {} · {} {} · {} {}",
      messages.context,
      messages.system_tokens,
      context.system_tokens,
      messages.context_tokens,
      context.context_tokens,
      messages.dropped,
      context.dropped
    ));
  }
  if let Some(usage) = view.usage {
    lines.push(format!(
      "{}: {} {} · {} {} · {} {}",
      messages.usage,
      messages.input_tokens,
      usage.input,
      messages.output_tokens,
      usage.output,
      messages.cache_read,
      usage.cache_read
    ));
  }
}

fn append_route(lines: &mut Vec<String>, view: &ActivityView, messages: ActivityMessages) {
  let Some(route) = &view.route else {
    lines.push(format!("{}: {}", messages.route, messages.none));
    return;
  };
  lines.push(format!(
    "{}: {}/{} · {}",
    messages.route,
    bounded_line(&route.selected.provider, messages),
    bounded_line(&route.selected.model, messages),
    bounded(&route.reason, messages)
  ));
  for fallback in route.fallbacks.iter().take(MAX_FALLBACKS) {
    lines.push(format!(
      "{}: {}/{} → {}/{} · {}",
      messages.fallback,
      bounded_line(&fallback.from.provider, messages),
      bounded_line(&fallback.from.model, messages),
      bounded_line(&fallback.to.provider, messages),
      bounded_line(&fallback.to.model, messages),
      bounded(&fallback.reason, messages)
    ));
  }
}

fn append_tools(lines: &mut Vec<String>, view: &ActivityView, messages: ActivityMessages) {
  for (index, tool) in view.tools.iter().take(MAX_TOOLS).enumerate() {
    lines.push(format!(
      "{} {}: {} · {} · call {}",
      messages.tool,
      index + 1,
      bounded_line(&tool.name, messages),
      tool_state_label(tool.state, messages),
      tool.call_id
    ));
    lines.push(format!(
      "  {}: {}",
      messages.arguments,
      bounded_json(&tool.arguments, messages.truncated_suffix)
    ));
    if let Some(approved_by) = &tool.approved_by {
      lines.push(format!(
        "  {}: {}",
        messages.approved_by,
        bounded(approved_by, messages)
      ));
    }
    if let Some(result) = &tool.result {
      lines.push(format!(
        "  {}: {} · invocation {} · {} {} ms",
        messages.result,
        if result.ok {
          messages.succeeded
        } else {
          messages.failed
        },
        result.invocation_id,
        messages.duration,
        result.duration_ms
      ));
      if !result.output.is_empty() {
        lines.push(format!(
          "  {}: {}",
          messages.output,
          bounded(&result.output, messages)
        ));
      }
      if let Some(error) = &result.error {
        lines.push(format!(
          "  {}: {}",
          messages.error,
          bounded(error, messages)
        ));
      }
      if result.truncated {
        lines.push(format!("  {}", messages.source_truncated));
      }
    }
  }
}

fn append_outcome(lines: &mut Vec<String>, view: &ActivityView, messages: ActivityMessages) {
  let Some(outcome) = &view.outcome else { return };
  let reason = match &outcome.stop_reason {
    StopReason::EndTurn => messages.end_turn.to_owned(),
    StopReason::ToolUse => messages.tool_use.to_owned(),
    StopReason::MaxTokens => messages.max_tokens.to_owned(),
    StopReason::Stop => messages.stopped.to_owned(),
    StopReason::Error(error) => format!("{}: {}", messages.error, bounded(error, messages)),
  };
  lines.push(format!("{}: {reason}", messages.outcome));
  if !outcome.text.is_empty() {
    lines.push(format!(
      "  {}: {}",
      messages.output,
      bounded(&outcome.text, messages)
    ));
  }
  lines.push(format!(
    "  {}: {} · {}: {}",
    messages.tool_calls,
    outcome.tool_calls.len(),
    messages.escalated,
    if outcome.escalated {
      messages.yes
    } else {
      messages.no
    }
  ));
  for call_id in outcome.tool_calls.iter().take(MAX_TOOL_CALL_IDS) {
    lines.push(format!("  call: {call_id}"));
  }
}

pub(crate) fn phase_label(phase: ActivityPhase, messages: ActivityMessages) -> &'static str {
  match phase {
    ActivityPhase::Preparing => messages.preparing,
    ActivityPhase::Streaming => messages.streaming,
    ActivityPhase::AwaitingApproval => messages.awaiting_approval,
    ActivityPhase::RunningTool => messages.running_tool,
    ActivityPhase::Superseded => messages.superseded,
    ActivityPhase::Completed => messages.completed,
    ActivityPhase::Failed => messages.failed,
  }
}

fn tool_state_label(state: ToolActivityState, messages: ActivityMessages) -> &'static str {
  match state {
    ToolActivityState::Requested => messages.requested,
    ToolActivityState::Approved => messages.approved,
    ToolActivityState::Running => messages.running_tool,
    ToolActivityState::Completed => messages.completed,
    ToolActivityState::Failed => messages.failed,
  }
}

fn bounded(value: &str, messages: ActivityMessages) -> String {
  let scanned = value.chars().take(FIELD_SCAN_LIMIT).collect::<String>();
  let input_truncated = value.chars().nth(FIELD_SCAN_LIMIT).is_some();
  let mut safe = terminal_safe(&scanned);
  let output_truncated = safe.chars().count() > FIELD_LIMIT;
  if output_truncated {
    safe = safe.chars().take(FIELD_LIMIT).collect();
  }
  if input_truncated || output_truncated {
    safe.push_str(messages.truncated_suffix);
  }
  safe
}

fn bounded_line(value: &str, messages: ActivityMessages) -> String {
  let scanned = value.chars().take(FIELD_SCAN_LIMIT).collect::<String>();
  let input_truncated = value.chars().nth(FIELD_SCAN_LIMIT).is_some();
  let mut safe = status_line_safe(&scanned);
  let output_truncated = safe.chars().count() > FIELD_LIMIT;
  if output_truncated {
    safe = safe.chars().take(FIELD_LIMIT).collect();
  }
  if input_truncated || output_truncated {
    safe.push_str(messages.truncated_suffix);
  }
  safe
}

fn enforce_total_budget(
  lines: Vec<String>,
  messages: ActivityMessages,
  mut truncated: bool,
) -> Vec<String> {
  let mut kept = Vec::new();
  let mut chars = 0usize;
  for line in lines {
    let line_chars = line.chars().count();
    if kept.len() >= MAX_DETAIL_LINES - 1 || chars.saturating_add(line_chars) > MAX_DETAIL_CHARS {
      truncated = true;
      break;
    }
    chars += line_chars;
    kept.push(line);
  }
  if truncated {
    if kept.len() >= MAX_DETAIL_LINES {
      kept.pop();
    }
    kept.push(messages.details_truncated.to_owned());
  }
  kept
}

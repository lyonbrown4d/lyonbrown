use agent_core::{
  RunStatus,
  events::{AgentEvent, ExecutionOutcome, ExecutionPhase, ModelIdentity},
};
use agent_protocol::v1::Event;
use serde_json::Value;

use super::{ActivityKind, ActivityState, ActivityStatus, values::*};

impl ActivityState {
  pub(super) fn project(&mut self, metadata: &Event, event: &AgentEvent) {
    match event {
      AgentEvent::PhaseChanged { phase, model } => {
        let item = self.item(metadata, "run", ActivityKind::Run);
        if let Some(model) = model
          && metadata.cursor >= item.model_cursor
        {
          item.target = short(&format!("{} / {}", model.provider, model.model), 100);
          item.model = Some(ModelIdentity {
            provider: safe(&model.provider),
            model: safe(&model.model),
            context_window: model.context_window,
          });
          item.model_cursor = metadata.cursor;
        }
        self.phase(metadata, phase_label(*phase), None);
      }
      AgentEvent::ToolStarted {
        call_id,
        name,
        arguments,
      } => self.tool_started(metadata, call_id, name, arguments),
      AgentEvent::ToolFinished {
        call_id,
        name,
        text,
        is_error,
      } => self.tool_finished(metadata, call_id, name, text, *is_error),
      AgentEvent::RunFinished { outcome, .. } => {
        self.phase(metadata, "", Some(outcome_status(*outcome)))
      }
      AgentEvent::RunStateChanged { status, error } => {
        self.phase(metadata, run_label(*status), Some(run_status(*status)));
        if !error.is_empty() {
          self.item(metadata, "run", ActivityKind::Run).result = bounded(error);
        }
      }
      AgentEvent::Compaction { active } => self.compaction(metadata, *active),
      AgentEvent::UsageUpdated { entry_id, usage } => {
        let observed = self
          .usage
          .entry((metadata.run_id.clone(), entry_id.clone()))
          .or_insert((metadata.cursor, *usage));
        if metadata.cursor >= observed.0 {
          *observed = (metadata.cursor, *usage);
        }
      }
      // Text content is presented by the transcript. Only the daemon's explicit
      // PhaseChanged event changes the displayed execution phase.
      AgentEvent::TextDelta { .. }
      | AgentEvent::ThinkingDelta { .. }
      | AgentEvent::QueueUpdated { .. } => {}
    }
  }

  fn tool_started(&mut self, metadata: &Event, call: &str, name: &str, arguments: &Value) {
    let run_ended = self.run_ended(&metadata.run_id);
    let item = self.item(metadata, &format!("tool:{call}"), ActivityKind::Tool);
    item.title = short(name, 80);
    item.started_at_ms = Some(
      item
        .started_at_ms
        .map_or(metadata.created_at_ms, |at| at.min(metadata.created_at_ms)),
    );
    item.target = target(arguments);
    item.arguments = details(arguments);
    if run_ended && item.status == ActivityStatus::Running {
      item.status = ActivityStatus::Uncertain;
      item.result = "执行已结束，尚未收到结果".into();
    }
  }

  fn tool_finished(
    &mut self,
    metadata: &Event,
    call: &str,
    name: &str,
    text: &str,
    is_error: bool,
  ) {
    let item = self.item(metadata, &format!("tool:{call}"), ActivityKind::Tool);
    if metadata.cursor < item.last_cursor {
      return;
    }
    item.last_cursor = metadata.cursor;
    item.title = short(name, 80);
    item.status = if is_error {
      ActivityStatus::Failed
    } else {
      ActivityStatus::Succeeded
    };
    item.finished_at_ms = Some(metadata.created_at_ms);
    item.result = bounded(text);
  }

  fn phase(&mut self, metadata: &Event, title: &str, next: Option<ActivityStatus>) {
    let item = self.item(metadata, "run", ActivityKind::Run);
    item.started_at_ms = Some(
      item
        .started_at_ms
        .map_or(metadata.created_at_ms, |at| at.min(metadata.created_at_ms)),
    );
    if metadata.cursor < item.last_cursor {
      return;
    }
    item.last_cursor = metadata.cursor;
    if item.status != ActivityStatus::Running
      && next.is_none_or(|next| next == ActivityStatus::Running)
    {
      return;
    }
    if !title.is_empty() {
      item.title = title.into();
    }
    if let Some(next) = next {
      item.status = next;
      if next != ActivityStatus::Running {
        item.finished_at_ms = Some(metadata.created_at_ms);
        item.title = match next {
          ActivityStatus::Succeeded => "执行已完成",
          ActivityStatus::Failed => "执行失败",
          ActivityStatus::Cancelled => "执行已取消",
          ActivityStatus::Uncertain => "执行结果待确认",
          ActivityStatus::Running => "正在执行",
        }
        .into();
      }
    }
    if next.is_some_and(|next| next != ActivityStatus::Running) {
      for pending in &mut self.items {
        if pending.run_id == metadata.run_id
          && pending.kind != ActivityKind::Run
          && pending.status == ActivityStatus::Running
        {
          pending.status = ActivityStatus::Uncertain;
          pending.result = "执行已结束，尚未收到结果".into();
        }
      }
    }
  }

  pub(super) fn run_ended(&self, run: &str) -> bool {
    self.items.iter().any(|item| {
      item.run_id == run && item.kind == ActivityKind::Run && item.status != ActivityStatus::Running
    })
  }
}

fn phase_label(phase: ExecutionPhase) -> &'static str {
  match phase {
    ExecutionPhase::Preparing => "正在准备执行",
    ExecutionPhase::ConnectingModel => "正在连接模型",
    ExecutionPhase::Generating => "模型正在生成",
    ExecutionPhase::Thinking => "模型正在思考",
    ExecutionPhase::Responding => "模型正在回复",
    ExecutionPhase::PreparingTool => "模型正在准备工具调用",
    ExecutionPhase::ExecutingTool => "正在执行工具",
    ExecutionPhase::AwaitingApproval => "等待工具授权",
    ExecutionPhase::Cancelling => "正在取消",
    ExecutionPhase::Compacting => "正在整理对话上下文",
  }
}

fn outcome_status(outcome: ExecutionOutcome) -> ActivityStatus {
  match outcome {
    ExecutionOutcome::Completed => ActivityStatus::Succeeded,
    ExecutionOutcome::Failed => ActivityStatus::Failed,
    ExecutionOutcome::Cancelled => ActivityStatus::Cancelled,
    ExecutionOutcome::Uncertain | ExecutionOutcome::Suspended => ActivityStatus::Uncertain,
  }
}

fn run_status(status: RunStatus) -> ActivityStatus {
  match status {
    RunStatus::Completed => ActivityStatus::Succeeded,
    RunStatus::Failed => ActivityStatus::Failed,
    RunStatus::Cancelled => ActivityStatus::Cancelled,
    RunStatus::Uncertain => ActivityStatus::Uncertain,
    RunStatus::Accepted
    | RunStatus::Running
    | RunStatus::AwaitingApproval
    | RunStatus::CancelRequested => ActivityStatus::Running,
  }
}

fn run_label(status: RunStatus) -> &'static str {
  match status {
    RunStatus::Accepted => "任务已接收",
    RunStatus::Running => "正在执行",
    RunStatus::AwaitingApproval => "等待工具授权",
    RunStatus::CancelRequested => "正在取消",
    _ => "",
  }
}

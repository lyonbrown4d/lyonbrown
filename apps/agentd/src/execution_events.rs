//! The daemon owns the public event vocabulary; clients never decode SDK data.
use agent_core::events::{AgentEvent, ExecutionOutcome, ExecutionPhase, ModelIdentity};
use agent_runtime::RuntimeEvent;

pub(crate) fn project(event: RuntimeEvent, model: &ModelIdentity, run_id: &str) -> Vec<AgentEvent> {
  let phase = |phase| AgentEvent::PhaseChanged {
    phase,
    model: Some(model.clone()),
  };
  match event {
    RuntimeEvent::RunStarted { .. } => vec![phase(ExecutionPhase::Preparing)],
    RuntimeEvent::ModelRequestStarted => vec![phase(ExecutionPhase::ConnectingModel)],
    RuntimeEvent::ModelResponseStarted => vec![phase(ExecutionPhase::Generating)],
    RuntimeEvent::PreparingTool => vec![phase(ExecutionPhase::PreparingTool)],
    RuntimeEvent::TextDelta { text } => vec![
      phase(ExecutionPhase::Responding),
      AgentEvent::TextDelta { text },
    ],
    RuntimeEvent::ThinkingDelta { text } => vec![
      phase(ExecutionPhase::Thinking),
      AgentEvent::ThinkingDelta { text },
    ],
    // The persistence adapter commits tool start/result facts with their
    // snapshot and outbox. The live adapter contributes only transient phase.
    RuntimeEvent::ToolStarted { .. } => vec![phase(ExecutionPhase::ExecutingTool)],
    RuntimeEvent::ToolFinished { .. } => Vec::new(),
    RuntimeEvent::RunFinished { outcome, .. } => vec![AgentEvent::RunFinished {
      run_id: run_id.into(),
      outcome: match outcome.as_str() {
        "completed" => ExecutionOutcome::Completed,
        "aborted" | "cancelled" => ExecutionOutcome::Cancelled,
        "suspended" => ExecutionOutcome::Suspended,
        "failed" | "declined" => ExecutionOutcome::Failed,
        _ => ExecutionOutcome::Uncertain,
      },
    }],
    RuntimeEvent::Compaction { active } => {
      let mut events = vec![AgentEvent::Compaction { active }];
      if active {
        events.insert(0, phase(ExecutionPhase::Compacting));
      }
      events
    }
  }
}

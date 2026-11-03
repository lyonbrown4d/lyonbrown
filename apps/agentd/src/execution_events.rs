//! The daemon owns the public event vocabulary; clients never decode SDK data.
use agent_core::events::{AgentEvent, ExecutionPhase, ModelIdentity};
use agent_runtime::RuntimeEvent;

pub(crate) fn project(
  event: RuntimeEvent,
  model: &ModelIdentity,
  _run_id: &str,
) -> Vec<AgentEvent> {
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
    // Completion is committed with the Rig snapshot, then rebased to the
    // daemon Run by RunPersistence. Do not emit a second terminal fact here.
    RuntimeEvent::RunFinished { .. } => Vec::new(),
    RuntimeEvent::Compaction { active } => {
      let mut events = vec![AgentEvent::Compaction { active }];
      if active {
        events.insert(0, phase(ExecutionPhase::Compacting));
      }
      events
    }
  }
}

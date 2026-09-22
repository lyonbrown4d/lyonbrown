use agent_core::{ModelCapabilities, StepOutcome, StopReason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateVerdict {
    Pass,
    Escalate(String),
}

pub trait QualityGate: Send + Sync {
    fn evaluate(&self, outcome: &StepOutcome, capabilities: &ModelCapabilities) -> GateVerdict;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicGate;

impl QualityGate for HeuristicGate {
    fn evaluate(&self, outcome: &StepOutcome, _capabilities: &ModelCapabilities) -> GateVerdict {
        match &outcome.stop_reason {
            StopReason::Error(message) => GateVerdict::Escalate(message.clone()),
            StopReason::EndTurn if outcome.text.trim().is_empty() => {
                GateVerdict::Escalate("model returned an empty final answer".into())
            }
            _ => GateVerdict::Pass,
        }
    }
}

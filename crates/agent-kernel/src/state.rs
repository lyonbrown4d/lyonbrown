use agent_core::{AgentEvent, StopReason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelState {
    Idle,
    PreparingContext,
    SelectingModel,
    ModelRunning,
    ToolRequired,
    ToolRunning,
    Observation,
    Completed,
    Failed { error: String },
    Cancelled,
}

impl KernelState {
    pub fn transition(&self, event: &AgentEvent) -> Result<Self, TransitionError> {
        if let AgentEvent::Error { message, .. } = event {
            return Ok(Self::Failed {
                error: message.clone(),
            });
        }

        if matches!(event, AgentEvent::TurnCancelled { .. })
            && matches!(
                self,
                Self::PreparingContext
                    | Self::SelectingModel
                    | Self::ModelRunning
                    | Self::ToolRequired
                    | Self::ToolRunning
                    | Self::Observation
            )
        {
            return Ok(Self::Cancelled);
        }

        let next = match (self, event) {
            (Self::Idle, AgentEvent::SessionStarted { .. })
            | (Self::Idle, AgentEvent::UserMessageReceived { .. })
            | (Self::Idle, AgentEvent::SessionClosed { .. }) => Self::Idle,
            (Self::Idle, AgentEvent::TurnStarted { .. }) => Self::PreparingContext,
            (Self::PreparingContext, AgentEvent::StepStarted { .. }) => Self::PreparingContext,
            (Self::PreparingContext, AgentEvent::ContextPrepared { .. }) => Self::SelectingModel,
            (Self::SelectingModel, AgentEvent::ModelSelected { .. }) => Self::ModelRunning,
            (Self::ModelRunning, AgentEvent::ModelStarted { .. })
            | (Self::ModelRunning, AgentEvent::ModelDelta { .. })
            | (Self::ModelRunning, AgentEvent::ModelCompleted { .. }) => Self::ModelRunning,
            (Self::ModelRunning, AgentEvent::ToolRequested { .. })
            | (Self::Observation, AgentEvent::ToolRequested { .. }) => Self::ToolRequired,
            (Self::ToolRequired, AgentEvent::ToolApproved { .. }) => Self::ToolRunning,
            (Self::ToolRunning, AgentEvent::ToolStarted { .. }) => Self::ToolRunning,
            (Self::ToolRunning, AgentEvent::ToolCompleted { .. }) => Self::Observation,
            (Self::ToolRequired, AgentEvent::ToolCompleted { .. }) => Self::Observation,
            (Self::Observation, AgentEvent::ObservationCreated { .. }) => Self::Observation,
            (
                Self::Observation,
                AgentEvent::StepCompleted {
                    outcome:
                        agent_core::StepOutcome {
                            stop_reason: StopReason::ToolUse,
                            ..
                        },
                    ..
                },
            ) => Self::PreparingContext,
            (
                Self::ModelRunning,
                AgentEvent::StepCompleted {
                    outcome:
                        agent_core::StepOutcome {
                            escalated: true, ..
                        },
                    ..
                },
            ) => Self::PreparingContext,
            (Self::ModelRunning, AgentEvent::StepCompleted { outcome, .. })
                if !matches!(outcome.stop_reason, StopReason::ToolUse) =>
            {
                Self::Completed
            }
            (Self::Completed, AgentEvent::TurnCompleted { .. }) => Self::Idle,
            _ => {
                return Err(TransitionError {
                    from: format!("{self:?}"),
                    event: event.kind(),
                });
            }
        };
        Ok(next)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("illegal transition from {from} on {event}")]
pub struct TransitionError {
    from: String,
    event: &'static str,
}

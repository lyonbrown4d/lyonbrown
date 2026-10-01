use agent_app::{AgentCommand, ApprovalDecision, ModelSelection};
use agent_core::ToolCallId;

#[test]
fn commands_keep_their_distinct_intent_and_values() {
  let request_id = ToolCallId::new();
  let commands = [
    AgentCommand::StartTurn {
      text: "start".into(),
    },
    AgentCommand::SteerCurrentTurn {
      text: "steer safely".into(),
    },
    AgentCommand::QueueFollowUp {
      text: "next turn".into(),
    },
    AgentCommand::CancelTurn,
    AgentCommand::ForceStop,
    AgentCommand::RespondApproval {
      request_id,
      decision: ApprovalDecision::DenyOnce,
    },
    AgentCommand::SelectSessionModel {
      selection: ModelSelection::Pinned {
        provider: "local".into(),
        model: "worker".into(),
      },
    },
  ];

  assert!(matches!(
    &commands[1],
    AgentCommand::SteerCurrentTurn { text } if text == "steer safely"
  ));
  assert!(matches!(
    &commands[2],
    AgentCommand::QueueFollowUp { text } if text == "next turn"
  ));
  assert!(matches!(
    &commands[5],
    AgentCommand::RespondApproval { request_id: id, decision: ApprovalDecision::DenyOnce }
      if *id == request_id
  ));
  assert!(matches!(
    &commands[6],
    AgentCommand::SelectSessionModel {
      selection: ModelSelection::Pinned { provider, model }
    } if provider == "local" && model == "worker"
  ));
}

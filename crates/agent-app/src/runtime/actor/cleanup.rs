use agent_tool::{ApprovalDecision as ToolDecision, ApprovalResponse};

use super::Actor;

impl Drop for Actor {
  fn drop(&mut self) {
    if let Some(cancellation) = &self.cancellation {
      cancellation.cancel();
    }
    if let Some(task) = self.turn_task.take() {
      task.abort();
    }
    if let Some(pending) = self.pending_approval.take() {
      let request_id = pending.request.request_id;
      pending.respond(ApprovalResponse {
        request_id,
        decision: ToolDecision::Deny,
      });
    }
  }
}

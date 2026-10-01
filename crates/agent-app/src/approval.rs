use agent_tool::{ApprovalPrompt, ApprovalRequest, ApprovalResponse};
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone)]
pub struct RuntimeApprovalPrompt {
  sender: mpsc::Sender<PendingApproval>,
}

#[derive(Debug)]
pub struct ApprovalInbox {
  receiver: mpsc::Receiver<PendingApproval>,
}

#[derive(Debug)]
pub(crate) struct PendingApproval {
  pub(crate) request: ApprovalRequest,
  response: oneshot::Sender<ApprovalResponse>,
}

impl PendingApproval {
  pub(crate) fn respond(self, response: ApprovalResponse) -> bool {
    self.response.send(response).is_ok()
  }

  pub(crate) fn is_closed(&self) -> bool {
    self.response.is_closed()
  }
}

impl ApprovalInbox {
  pub(crate) async fn recv(&mut self) -> Option<PendingApproval> {
    self.receiver.recv().await
  }
}

pub fn approval_channel(capacity: usize) -> (RuntimeApprovalPrompt, ApprovalInbox) {
  let (sender, receiver) = mpsc::channel(capacity.max(1));
  (RuntimeApprovalPrompt { sender }, ApprovalInbox { receiver })
}

#[async_trait]
impl ApprovalPrompt for RuntimeApprovalPrompt {
  async fn prompt(&self, request: ApprovalRequest) -> Option<ApprovalResponse> {
    let (response, receiver) = oneshot::channel();
    self
      .sender
      .send(PendingApproval { request, response })
      .await
      .ok()?;
    receiver.await.ok()
  }
}

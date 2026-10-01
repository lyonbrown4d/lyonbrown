use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agent_core::{RiskLevel, ToolCallId, ToolPreview};
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
  Allow,
  AllowSession,
  Deny,
  DenyAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
  pub request_id: ToolCallId,
  pub preview: ToolPreview,
  pub deadline: Option<Instant>,
}

impl ApprovalRequest {
  pub fn new(id: ToolCallId, preview: ToolPreview) -> Self {
    Self {
      request_id: id,
      preview,
      deadline: None,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovalResponse {
  pub request_id: ToolCallId,
  pub decision: ApprovalDecision,
}

#[async_trait]
pub trait ApprovalPolicy: Send + Sync {
  async fn approve(&self, request: ApprovalRequest) -> ApprovalResponse;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAllApproval;

#[async_trait]
impl ApprovalPolicy for AllowAllApproval {
  async fn approve(&self, request: ApprovalRequest) -> ApprovalResponse {
    response(request.request_id, ApprovalDecision::Allow)
  }
}

#[async_trait]
pub trait ApprovalPrompt: Send + Sync {
  /// `None` means the responder disconnected and must be treated as denial.
  async fn prompt(&self, request: ApprovalRequest) -> Option<ApprovalResponse>;
}

enum ApprovalMode {
  Interactive {
    prompt: Arc<dyn ApprovalPrompt>,
    timeout: Option<Duration>,
  },
  ApproveAll,
  DenyAll,
  NonInteractive,
}

#[derive(Default)]
struct SessionState {
  allowed_tools: HashSet<String>,
  deny_all_rest: bool,
}

pub struct SessionApprovalPolicy {
  mode: ApprovalMode,
  state: Mutex<SessionState>,
}

impl SessionApprovalPolicy {
  pub fn interactive(prompt: Arc<dyn ApprovalPrompt>) -> Self {
    Self::new(ApprovalMode::Interactive {
      prompt,
      timeout: None,
    })
  }

  pub fn interactive_with_timeout(prompt: Arc<dyn ApprovalPrompt>, timeout: Duration) -> Self {
    Self::new(ApprovalMode::Interactive {
      prompt,
      timeout: Some(timeout),
    })
  }

  pub fn approve_all() -> Self {
    Self::new(ApprovalMode::ApproveAll)
  }

  pub fn deny_all() -> Self {
    Self::new(ApprovalMode::DenyAll)
  }

  pub fn non_interactive() -> Self {
    Self::new(ApprovalMode::NonInteractive)
  }

  fn new(mode: ApprovalMode) -> Self {
    Self {
      mode,
      state: Mutex::new(SessionState::default()),
    }
  }
}

#[async_trait]
impl ApprovalPolicy for SessionApprovalPolicy {
  async fn approve(&self, mut request: ApprovalRequest) -> ApprovalResponse {
    let request_id = request.request_id;
    let tool = request.preview.tool.clone();
    if request.preview.risk == RiskLevel::Read {
      return response(request_id, ApprovalDecision::Allow);
    }
    {
      let state = lock_state(&self.state);
      if state.deny_all_rest {
        return response(request_id, ApprovalDecision::Deny);
      }
      if state.allowed_tools.contains(&request.preview.tool) {
        return response(request_id, ApprovalDecision::Allow);
      }
    }

    let decision = match &self.mode {
      ApprovalMode::Interactive { prompt, timeout } => {
        request.deadline = timeout.map(|duration| Instant::now() + duration);
        prompt_decision(prompt, request, *timeout).await
      }
      ApprovalMode::ApproveAll => ApprovalDecision::Allow,
      ApprovalMode::DenyAll | ApprovalMode::NonInteractive => ApprovalDecision::Deny,
    };
    let mut state = lock_state(&self.state);
    match decision {
      ApprovalDecision::AllowSession => {
        state.allowed_tools.insert(tool);
      }
      ApprovalDecision::DenyAll => state.deny_all_rest = true,
      ApprovalDecision::Allow | ApprovalDecision::Deny => {}
    }
    response(request_id, decision)
  }
}

fn lock_state(state: &Mutex<SessionState>) -> std::sync::MutexGuard<'_, SessionState> {
  state
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn prompt_decision(
  prompt: &Arc<dyn ApprovalPrompt>,
  request: ApprovalRequest,
  timeout: Option<Duration>,
) -> ApprovalDecision {
  let request_id = request.request_id;
  let response = match timeout {
    Some(timeout) => tokio::time::timeout(timeout, prompt.prompt(request))
      .await
      .ok()
      .flatten(),
    None => prompt.prompt(request).await,
  };
  match response {
    Some(response) if response.request_id == request_id => response.decision,
    Some(_) | None => ApprovalDecision::Deny,
  }
}

fn response(request_id: ToolCallId, decision: ApprovalDecision) -> ApprovalResponse {
  ApprovalResponse {
    request_id,
    decision,
  }
}

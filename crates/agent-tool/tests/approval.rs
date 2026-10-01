use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_core::{RiskLevel, ToolCallId, ToolPreview};
use agent_tool::{
  ApprovalDecision, ApprovalPolicy, ApprovalPrompt, ApprovalRequest, ApprovalResponse,
  SessionApprovalPolicy,
};
use async_trait::async_trait;

struct ScriptedPrompt {
  decisions: Mutex<VecDeque<Option<ApprovalDecision>>>,
}

impl ScriptedPrompt {
  fn new(decisions: impl IntoIterator<Item = Option<ApprovalDecision>>) -> Self {
    Self {
      decisions: Mutex::new(decisions.into_iter().collect()),
    }
  }
}

#[async_trait]
impl ApprovalPrompt for ScriptedPrompt {
  async fn prompt(&self, request: ApprovalRequest) -> Option<ApprovalResponse> {
    self
      .decisions
      .lock()
      .unwrap()
      .pop_front()
      .flatten()
      .map(|decision| ApprovalResponse {
        request_id: request.request_id,
        decision,
      })
  }
}

struct PendingPrompt;

#[async_trait]
impl ApprovalPrompt for PendingPrompt {
  async fn prompt(&self, _request: ApprovalRequest) -> Option<ApprovalResponse> {
    std::future::pending().await
  }
}

struct WrongIdPrompt;

#[async_trait]
impl ApprovalPrompt for WrongIdPrompt {
  async fn prompt(&self, _request: ApprovalRequest) -> Option<ApprovalResponse> {
    Some(ApprovalResponse {
      request_id: ToolCallId::new(),
      decision: ApprovalDecision::Allow,
    })
  }
}

fn request(name: &str, risk: RiskLevel) -> ApprovalRequest {
  ApprovalRequest::new(
    ToolCallId::new(),
    ToolPreview {
      tool: name.into(),
      action: "test action".into(),
      target: name.into(),
      impact: "test impact".into(),
      risk,
      sandbox: agent_core::SandboxStatus::Unknown,
      working_directory: None,
      required_secrets: vec![],
    },
  )
}

#[tokio::test]
async fn read_tools_are_always_allowed_without_prompting() {
  let policy = SessionApprovalPolicy::interactive(Arc::new(ScriptedPrompt::new([])));
  let request = request("read_file", RiskLevel::Read);
  let id = request.request_id;

  assert_eq!(
    policy.approve(request).await,
    ApprovalResponse {
      request_id: id,
      decision: ApprovalDecision::Allow,
    }
  );
}

#[tokio::test]
async fn allow_once_does_not_change_later_decisions() {
  let prompt = Arc::new(ScriptedPrompt::new([
    Some(ApprovalDecision::Allow),
    Some(ApprovalDecision::Deny),
  ]));
  let policy = SessionApprovalPolicy::interactive(prompt);

  assert_eq!(
    policy
      .approve(request("write_file", RiskLevel::Write))
      .await
      .decision,
    ApprovalDecision::Allow
  );
  assert_eq!(
    policy
      .approve(request("write_file", RiskLevel::Write))
      .await
      .decision,
    ApprovalDecision::Deny
  );
}

#[tokio::test]
async fn allow_session_is_remembered_per_tool_name() {
  let prompt = Arc::new(ScriptedPrompt::new([Some(ApprovalDecision::AllowSession)]));
  let policy = SessionApprovalPolicy::interactive(prompt);

  assert_eq!(
    policy
      .approve(request("write_file", RiskLevel::Write))
      .await
      .decision,
    ApprovalDecision::AllowSession
  );
  assert_eq!(
    policy
      .approve(request("write_file", RiskLevel::Write))
      .await
      .decision,
    ApprovalDecision::Allow
  );
}

#[tokio::test]
async fn deny_all_is_remembered_for_later_high_risk_tools() {
  let prompt = Arc::new(ScriptedPrompt::new([Some(ApprovalDecision::DenyAll)]));
  let policy = SessionApprovalPolicy::interactive(prompt);

  assert_eq!(
    policy
      .approve(request("shell", RiskLevel::Execute))
      .await
      .decision,
    ApprovalDecision::DenyAll
  );
  assert_eq!(
    policy
      .approve(request("write_file", RiskLevel::Write))
      .await
      .decision,
    ApprovalDecision::Deny
  );
}

#[tokio::test]
async fn prompt_disconnect_fails_closed() {
  let policy = SessionApprovalPolicy::interactive(Arc::new(ScriptedPrompt::new([None])));

  assert_eq!(
    policy
      .approve(request("shell", RiskLevel::Execute))
      .await
      .decision,
    ApprovalDecision::Deny
  );
}

#[tokio::test]
async fn mismatched_response_id_fails_closed() {
  let policy = SessionApprovalPolicy::interactive(Arc::new(WrongIdPrompt));

  assert_eq!(
    policy
      .approve(request("shell", RiskLevel::Execute))
      .await
      .decision,
    ApprovalDecision::Deny
  );
}

#[tokio::test]
async fn non_interactive_sessions_fail_closed_for_high_risk_tools() {
  let policy = SessionApprovalPolicy::non_interactive();

  assert_eq!(
    policy
      .approve(request("shell", RiskLevel::Execute))
      .await
      .decision,
    ApprovalDecision::Deny
  );
}

#[tokio::test]
async fn prompt_timeout_fails_closed() {
  let policy = SessionApprovalPolicy::interactive_with_timeout(
    Arc::new(PendingPrompt),
    Duration::from_millis(10),
  );

  assert_eq!(
    policy
      .approve(request("shell", RiskLevel::Execute))
      .await
      .decision,
    ApprovalDecision::Deny
  );
}

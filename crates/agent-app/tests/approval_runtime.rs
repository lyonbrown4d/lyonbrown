use std::sync::Arc;
use std::time::Duration;

use agent_app::{
  AgentCommand, ApprovalDecision, RuntimeCommandError, approval_channel,
  spawn_runtime_with_approvals,
};
use agent_core::{RiskLevel, SandboxStatus, ToolCallId};
use agent_kernel::Kernel;
use agent_model::ModelRegistry;
use agent_tool::{ApprovalPrompt, SessionApprovalPolicy, ShellTool, ToolRegistry};
use provider_mock::{MockProvider, ScriptedStep};
use serde_json::json;
use tokio::sync::watch;

fn approval_kernel(prompt: agent_app::RuntimeApprovalPrompt) -> Kernel {
  let providers = ModelRegistry::new();
  providers.register(Arc::new(MockProvider::scripted(vec![
    ScriptedStep::tool_call(
      "shell",
      json!({
        "command": "echo approval-test",
        "env": {"TOKEN": "secret:env:APP_TOKEN"}
      }),
    ),
    ScriptedStep::text("done"),
  ])));
  let tools = ToolRegistry::new();
  tools.register(Arc::new(ShellTool::new()));
  Kernel::builder()
    .providers(providers)
    .tools(tools)
    .approval(SessionApprovalPolicy::interactive_with_timeout(
      Arc::new(prompt),
      Duration::from_secs(5),
    ))
    .build()
}

async fn wait_for(
  snapshots: &mut watch::Receiver<agent_app::RuntimeSnapshot>,
  predicate: impl Fn(&agent_app::RuntimeSnapshot) -> bool,
) {
  let result = tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      if predicate(&snapshots.borrow()) {
        return;
      }
      snapshots.changed().await.unwrap();
    }
  })
  .await;
  assert!(
    result.is_ok(),
    "snapshot timeout: {:#?}",
    snapshots.borrow()
  );
}

#[tokio::test]
async fn pending_request_projects_preview_and_accepts_matching_response() {
  let (prompt, inbox) = approval_channel(2);
  let kernel = approval_kernel(prompt);
  let (controller, task) = spawn_runtime_with_approvals(kernel, inbox, 4);
  let mut snapshots = controller.subscribe();

  controller
    .execute(AgentCommand::StartTurn {
      text: "run tool".into(),
    })
    .await
    .unwrap();
  wait_for(&mut snapshots, |snapshot| {
    snapshot
      .approval
      .as_ref()
      .is_some_and(|approval| approval.preview.is_some())
  })
  .await;

  let approval = snapshots.borrow().approval.clone().unwrap();
  let preview = approval.preview.unwrap();
  assert_eq!(preview.action, "Execute shell command");
  assert_eq!(preview.target, "echo approval-test");
  assert_eq!(preview.risk, RiskLevel::Execute);
  assert_eq!(preview.sandbox, SandboxStatus::HostUnsandboxed);
  assert!(preview.working_directory.is_some());
  assert_eq!(preview.required_secrets[0].0, "env:APP_TOKEN");
  assert!(approval.deadline.is_some());

  let wrong_id = ToolCallId::new();
  assert_eq!(
    controller
      .execute(AgentCommand::RespondApproval {
        request_id: wrong_id,
        decision: ApprovalDecision::DenyOnce,
      })
      .await,
    Err(RuntimeCommandError::ApprovalRequestMismatch {
      expected: approval.request_id,
      received: wrong_id,
    })
  );
  controller
    .execute(AgentCommand::RespondApproval {
      request_id: approval.request_id,
      decision: ApprovalDecision::DenyOnce,
    })
    .await
    .unwrap();
  wait_for(&mut snapshots, |snapshot| {
    snapshot.turns[0].activities[0].tools[0].state == agent_app::ToolActivityState::Failed
  })
  .await;
  assert_eq!(
    controller
      .execute(AgentCommand::RespondApproval {
        request_id: approval.request_id,
        decision: ApprovalDecision::DenyOnce,
      })
      .await,
    Err(RuntimeCommandError::NoPendingApproval)
  );

  drop(controller);
  task.join().await.unwrap();
}

#[tokio::test]
async fn cancel_denies_pending_approval_then_cancels_the_turn() {
  let (prompt, inbox) = approval_channel(2);
  let (controller, task) = spawn_runtime_with_approvals(approval_kernel(prompt), inbox, 4);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "run tool".into(),
    })
    .await
    .unwrap();
  wait_for(&mut snapshots, |snapshot| {
    snapshot
      .approval
      .as_ref()
      .is_some_and(|approval| approval.preview.is_some())
  })
  .await;

  controller.execute(AgentCommand::CancelTurn).await.unwrap();
  wait_for(&mut snapshots, |snapshot| {
    snapshot.approval.is_none()
      && snapshot
        .turns
        .last()
        .is_some_and(|turn| turn.status == agent_app::TurnStatus::Cancelled)
  })
  .await;
  drop(controller);
  task.join().await.unwrap();
}

#[tokio::test]
async fn dropped_inbox_and_stopped_actor_fail_closed() {
  let (prompt, inbox) = approval_channel(1);
  drop(inbox);
  let request = agent_tool::ApprovalRequest::new(
    ToolCallId::new(),
    agent_core::ToolPreview {
      tool: "shell".into(),
      action: "execute".into(),
      target: "echo no".into(),
      impact: "test".into(),
      risk: RiskLevel::Execute,
      sandbox: SandboxStatus::HostUnsandboxed,
      working_directory: None,
      required_secrets: vec![],
    },
  );
  assert!(prompt.prompt(request).await.is_none());

  let (prompt, inbox) = approval_channel(1);
  let (controller, task) = spawn_runtime_with_approvals(approval_kernel(prompt), inbox, 2);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn { text: "run".into() })
    .await
    .unwrap();
  wait_for(&mut snapshots, |snapshot| snapshot.approval.is_some()).await;
  drop(snapshots);
  drop(controller);
  tokio::time::timeout(Duration::from_secs(2), task.join())
    .await
    .expect("actor must deny and stop instead of hanging")
    .unwrap();
}

#[tokio::test]
async fn timed_out_approval_is_cleared_before_the_next_request() {
  let (prompt, inbox) = approval_channel(2);
  let providers = ModelRegistry::new();
  providers.register(Arc::new(MockProvider::scripted(vec![
    ScriptedStep::tool_call("shell", json!({"command": "echo first"})),
    ScriptedStep::tool_call("shell", json!({"command": "echo second"})),
    ScriptedStep::text("done"),
  ])));
  let tools = ToolRegistry::new();
  tools.register(Arc::new(ShellTool::new()));
  let kernel = Kernel::builder()
    .providers(providers)
    .tools(tools)
    .approval(SessionApprovalPolicy::interactive_with_timeout(
      Arc::new(prompt),
      Duration::from_millis(100),
    ))
    .build();
  let (controller, task) = spawn_runtime_with_approvals(kernel, inbox, 4);
  let mut snapshots = controller.subscribe();

  controller
    .execute(AgentCommand::StartTurn { text: "run".into() })
    .await
    .unwrap();
  wait_for(&mut snapshots, |snapshot| {
    snapshot
      .approval
      .as_ref()
      .and_then(|approval| approval.preview.as_ref())
      .is_some_and(|preview| preview.target == "echo second")
  })
  .await;
  let second = snapshots.borrow().approval.as_ref().unwrap().request_id;
  controller
    .execute(AgentCommand::RespondApproval {
      request_id: second,
      decision: ApprovalDecision::DenyOnce,
    })
    .await
    .unwrap();

  drop(controller);
  task.join().await.unwrap();
}

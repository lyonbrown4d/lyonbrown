use std::sync::Arc;
use std::time::Duration;

use agent_app::{
  AgentCommand, RuntimeApprovalPrompt, RuntimeCommandError, RuntimePhase, TurnStatus,
  approval_channel, spawn_runtime, spawn_runtime_with_approvals,
};
use agent_kernel::Kernel;
use agent_model::ModelRegistry;
use agent_tool::{
  ApprovalDecision as ToolApprovalDecision, ApprovalPrompt, ApprovalRequest, ApprovalResponse,
  SessionApprovalPolicy, ShellTool, ToolRegistry,
};
use async_trait::async_trait;
use provider_mock::{MockProvider, ScriptedStep};
use serde_json::json;
use tokio::sync::watch;

fn pending_kernel() -> Kernel {
  let providers = ModelRegistry::new();
  providers.register(Arc::new(MockProvider::scripted(vec![
    ScriptedStep::pending(),
  ])));
  Kernel::builder().providers(providers).build()
}

fn approval_kernel(prompt: Arc<dyn ApprovalPrompt>) -> Kernel {
  let providers = ModelRegistry::new();
  providers.register(Arc::new(MockProvider::scripted(vec![
    ScriptedStep::tool_call("shell", json!({"command": "echo shutdown"})),
  ])));
  let tools = ToolRegistry::new();
  tools.register(Arc::new(ShellTool::new()));
  Kernel::builder()
    .providers(providers)
    .tools(tools)
    .approval(SessionApprovalPolicy::interactive_with_timeout(
      prompt,
      Duration::from_secs(5),
    ))
    .build()
}

struct ObservedPrompt {
  inner: RuntimeApprovalPrompt,
  decisions: tokio::sync::mpsc::UnboundedSender<ApprovalResponse>,
}

#[async_trait]
impl ApprovalPrompt for ObservedPrompt {
  async fn prompt(&self, request: ApprovalRequest) -> Option<ApprovalResponse> {
    let response = self.inner.prompt(request).await;
    if let Some(response) = response {
      let _ = self.decisions.send(response);
    }
    response
  }
}

async fn wait_for_phase(
  snapshots: &mut watch::Receiver<agent_app::RuntimeSnapshot>,
  phase: RuntimePhase,
) {
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if snapshots.borrow().phase == phase {
        return;
      }
      snapshots.changed().await.unwrap();
    }
  })
  .await
  .expect("runtime did not reach the expected phase");
}

#[tokio::test]
async fn shutdown_joins_while_controller_clones_are_still_alive() {
  let (controller, task) = spawn_runtime(Kernel::builder().build(), 4);
  let retained_controller = controller.clone();
  let shutdown = task.shutdown_handle().clone();

  shutdown.request_shutdown();
  let command_result = tokio::time::timeout(
    Duration::from_secs(2),
    retained_controller.execute(AgentCommand::StartTurn {
      text: "too late".into(),
    }),
  )
  .await
  .expect("commands issued after shutdown must be rejected promptly");
  assert_eq!(command_result, Err(RuntimeCommandError::RuntimeStopped));
  tokio::time::timeout(Duration::from_secs(2), task.join())
    .await
    .expect("shutdown must not wait for controllers to disconnect")
    .unwrap();
}

#[tokio::test]
async fn shutdown_cancels_an_active_turn_and_discards_follow_ups() {
  let (controller, task) = spawn_runtime(pending_kernel(), 4);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "wait".into(),
    })
    .await
    .unwrap();
  wait_for_phase(&mut snapshots, RuntimePhase::Streaming).await;
  controller
    .execute(AgentCommand::QueueFollowUp {
      text: "must not run".into(),
    })
    .await
    .unwrap();

  task.shutdown_handle().request_shutdown();
  tokio::time::timeout(Duration::from_secs(2), task.join())
    .await
    .expect("active turn must observe runtime shutdown")
    .unwrap();

  let snapshot = snapshots.borrow();
  assert!(snapshot.queued_follow_ups.is_empty());
  assert_eq!(snapshot.turns.len(), 1);
  assert_eq!(snapshot.turns[0].status, TurnStatus::Cancelled);
}

#[tokio::test]
async fn abort_reaches_the_actor_and_join_never_hangs() {
  let (controller, task) = spawn_runtime(pending_kernel(), 4);
  controller
    .execute(AgentCommand::StartTurn {
      text: "wait".into(),
    })
    .await
    .unwrap();

  task.shutdown_handle().abort();
  let outcome = tokio::time::timeout(Duration::from_secs(2), task.join())
    .await
    .expect("aborted runtime join must resolve");
  assert!(
    outcome.is_err(),
    "aborting reports that cleanup was skipped"
  );
  assert_eq!(
    controller.execute(AgentCommand::CancelTurn).await,
    Err(RuntimeCommandError::RuntimeStopped)
  );
}

#[tokio::test]
async fn runtime_task_abort_still_terminates_the_actor() {
  let (_controller, task) = spawn_runtime(pending_kernel(), 4);

  task.abort();
  let outcome = tokio::time::timeout(Duration::from_secs(2), task.join())
    .await
    .expect("RuntimeTask::abort must make join resolve");
  assert!(outcome.is_err());
}

#[tokio::test]
async fn shutdown_denies_and_clears_a_pending_approval() {
  let (prompt, inbox) = approval_channel(1);
  let (decision_tx, mut decision_rx) = tokio::sync::mpsc::unbounded_channel();
  let observed = ObservedPrompt {
    inner: prompt,
    decisions: decision_tx,
  };
  let (controller, task) =
    spawn_runtime_with_approvals(approval_kernel(Arc::new(observed)), inbox, 4);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "run tool".into(),
    })
    .await
    .unwrap();
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if snapshots.borrow().approval.is_some() {
        break;
      }
      snapshots.changed().await.unwrap();
    }
  })
  .await
  .expect("approval was not projected");

  task.shutdown_handle().request_shutdown();
  tokio::time::timeout(Duration::from_secs(2), task.join())
    .await
    .expect("shutdown must not wait for the approval deadline")
    .unwrap();

  let snapshot = snapshots.borrow();
  assert!(snapshot.approval.is_none());
  assert_eq!(snapshot.turns[0].status, TurnStatus::Cancelled);
  assert_eq!(
    decision_rx.try_recv().unwrap().decision,
    ToolApprovalDecision::Deny
  );
}

#[tokio::test]
async fn shutdown_is_not_starved_by_a_continuous_command_stream() {
  let (controller, task) = spawn_runtime(pending_kernel(), 4);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "wait".into(),
    })
    .await
    .unwrap();
  wait_for_phase(&mut snapshots, RuntimePhase::Streaming).await;

  let mut senders = Vec::new();
  for _ in 0..8 {
    let controller = controller.clone();
    senders.push(tokio::spawn(async move {
      loop {
        if controller
          .execute(AgentCommand::QueueFollowUp {
            text: "flood".into(),
          })
          .await
          .is_err()
        {
          break;
        }
      }
    }));
  }
  tokio::task::yield_now().await;

  task.shutdown_handle().request_shutdown();
  tokio::time::timeout(Duration::from_secs(2), task.join())
    .await
    .expect("command traffic must not starve actor shutdown")
    .unwrap();
  for sender in senders {
    tokio::time::timeout(Duration::from_secs(2), sender)
      .await
      .expect("command sender must observe the closed runtime")
      .unwrap();
  }
}

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use agent_app::{
  AgentCommand, RuntimeCommandError, RuntimePhase, TurnStatus, UnsupportedCommand, spawn_runtime,
};
use agent_core::{Locality, ModelCapabilities, ModelTier, StopReason, TokenUsage, ToolCallId};
use agent_kernel::Kernel;
use agent_model::{
  ModelError, ModelEvent, ModelProvider, ModelRegistry, ModelRequest, ModelStream,
};
use async_trait::async_trait;
use futures::stream;
use provider_mock::{MockProvider, ScriptedStep};
use tokio::sync::{Notify, watch};

fn kernel_with(provider: Arc<dyn ModelProvider>) -> Kernel {
  let providers = ModelRegistry::new();
  providers.register(provider);
  Kernel::builder().providers(providers).build()
}

async fn wait_for(
  snapshots: &mut watch::Receiver<agent_app::RuntimeSnapshot>,
  predicate: impl Fn(&agent_app::RuntimeSnapshot) -> bool,
) {
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if predicate(&snapshots.borrow()) {
        return;
      }
      snapshots.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn start_turn_runs_kernel_and_publishes_projected_snapshot() {
  let kernel = kernel_with(Arc::new(MockProvider::scripted(vec![ScriptedStep::text(
    "answer",
  )])));
  let (controller, task) = spawn_runtime(kernel, 4);
  let mut snapshots = controller.subscribe();

  controller
    .execute(AgentCommand::StartTurn {
      text: "question".into(),
    })
    .await
    .unwrap();
  wait_for(&mut snapshots, |snapshot| {
    snapshot.phase == RuntimePhase::Completed
  })
  .await;

  let snapshot = snapshots.borrow().clone();
  assert_eq!(snapshot.turns[0].user_text, "question");
  assert_eq!(snapshot.turns[0].assistant_text, "answer");
  drop(controller);
  task.join().await.unwrap();
}

#[tokio::test]
async fn startup_rebuilds_the_snapshot_from_the_kernel_event_log() {
  let mut kernel = kernel_with(Arc::new(MockProvider::scripted(vec![ScriptedStep::text(
    "earlier answer",
  )])));
  kernel.run_turn("earlier question").await.unwrap();

  let (controller, task) = spawn_runtime(kernel, 4);
  let snapshot = controller.snapshot();
  assert_eq!(snapshot.turns.len(), 1);
  assert_eq!(snapshot.turns[0].user_text, "earlier question");
  assert_eq!(snapshot.turns[0].assistant_text, "earlier answer");
  assert_eq!(snapshot.turns[0].status, TurnStatus::Completed);

  drop(controller);
  task.join().await.unwrap();
}

#[tokio::test]
async fn queued_follow_up_starts_only_after_the_active_turn_finishes() {
  let provider = Arc::new(PausedProvider::new());
  let kernel = kernel_with(provider.clone());
  let (controller, task) = spawn_runtime(kernel, 4);
  let mut snapshots = controller.subscribe();

  controller
    .execute(AgentCommand::StartTurn {
      text: "first".into(),
    })
    .await
    .unwrap();
  provider.wait_until_called().await;
  controller
    .execute(AgentCommand::QueueFollowUp {
      text: "second".into(),
    })
    .await
    .unwrap();
  assert_eq!(controller.snapshot().queued_follow_ups, ["second"]);
  assert_eq!(provider.calls(), 1);
  provider.release();

  wait_for(&mut snapshots, |snapshot| {
    snapshot.turns.len() == 2 && snapshot.turns[1].status == TurnStatus::Completed
  })
  .await;
  assert_eq!(provider.calls(), 2);
  assert!(snapshots.borrow().queued_follow_ups.is_empty());
  assert_eq!(snapshots.borrow().turns[1].user_text, "second");
  drop(controller);
  task.join().await.unwrap();
}

#[tokio::test]
async fn steering_reaches_the_running_kernel_at_a_safe_boundary() {
  let provider = Arc::new(PausedProvider::new());
  let kernel = kernel_with(provider.clone());
  let (controller, task) = spawn_runtime(kernel, 4);
  let mut snapshots = controller.subscribe();

  controller
    .execute(AgentCommand::StartTurn {
      text: "inspect".into(),
    })
    .await
    .unwrap();
  provider.wait_until_called().await;
  controller
    .execute(AgentCommand::SteerCurrentTurn {
      text: "focus".into(),
    })
    .await
    .unwrap();
  provider.release();

  wait_for(&mut snapshots, |snapshot| {
    snapshot.phase == RuntimePhase::Completed
  })
  .await;
  assert_eq!(provider.calls(), 2);
  assert_eq!(snapshots.borrow().turns[0].steering, ["focus"]);
  drop(controller);
  task.join().await.unwrap();
}

#[tokio::test]
async fn cancellation_reaches_kernel_and_projects_cancelled_turn() {
  let kernel = kernel_with(Arc::new(MockProvider::scripted(vec![
    ScriptedStep::pending(),
  ])));
  let (controller, task) = spawn_runtime(kernel, 4);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "wait".into(),
    })
    .await
    .unwrap();
  wait_for(&mut snapshots, |snapshot| {
    snapshot.phase == RuntimePhase::Streaming
  })
  .await;

  controller.execute(AgentCommand::CancelTurn).await.unwrap();
  wait_for(&mut snapshots, |snapshot| {
    snapshot
      .turns
      .last()
      .is_some_and(|turn| turn.status == TurnStatus::Cancelled)
  })
  .await;
  drop(controller);
  task.join().await.unwrap();
}

#[tokio::test]
async fn unavailable_kernel_controls_return_typed_errors() {
  let (controller, task) = spawn_runtime(Kernel::builder().build(), 4);
  assert_eq!(
    controller
      .execute(AgentCommand::RespondApproval {
        request_id: ToolCallId::new(),
        decision: agent_app::ApprovalDecision::DenyOnce,
      })
      .await,
    Err(RuntimeCommandError::NoPendingApproval)
  );
  assert_eq!(
    controller
      .execute(AgentCommand::SelectSessionModel {
        selection: agent_app::ModelSelection::Auto,
      })
      .await,
    Err(RuntimeCommandError::Unsupported(
      UnsupportedCommand::SelectSessionModel
    ))
  );
  drop(controller);
  task.join().await.unwrap();
}

struct PausedProvider {
  calls: AtomicUsize,
  release_first: Notify,
}

impl PausedProvider {
  fn new() -> Self {
    Self {
      calls: AtomicUsize::new(0),
      release_first: Notify::new(),
    }
  }

  fn calls(&self) -> usize {
    self.calls.load(Ordering::Acquire)
  }

  async fn wait_until_called(&self) {
    tokio::time::timeout(Duration::from_secs(1), async {
      while self.calls() == 0 {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
  }

  fn release(&self) {
    self.release_first.notify_one();
  }
}

#[async_trait]
impl ModelProvider for PausedProvider {
  fn provider_id(&self) -> &'static str {
    "paused"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Ok(vec![ModelCapabilities {
      provider: "paused".into(),
      model: "test".into(),
      tier: ModelTier::Worker,
      locality: Locality::Local,
      context_window: 32_768,
      max_output_tokens: 1_024,
      supports_tools: true,
      supports_streaming: true,
      supports_json: true,
      supports_thinking: false,
      cost_per_mtok_in: 0.0,
      cost_per_mtok_out: 0.0,
    }])
  }

  async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
    let call = self.calls.fetch_add(1, Ordering::AcqRel);
    if call == 0 {
      self.release_first.notified().await;
    }
    Ok(ModelStream {
      model: "test".into(),
      events: Box::pin(stream::iter(vec![
        Ok(ModelEvent::Delta(format!("answer-{call}"))),
        Ok(ModelEvent::Usage(TokenUsage::default())),
        Ok(ModelEvent::Done {
          stop_reason: StopReason::EndTurn,
        }),
      ])),
    })
  }
}

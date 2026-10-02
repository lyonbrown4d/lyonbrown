use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agent_app::{AgentCommand, ModelSelection, RuntimePhase, TurnStatus, spawn_runtime};
use agent_core::{Locality, ModelCapabilities, ModelTier, StopReason, TokenUsage};
use agent_kernel::Kernel;
use agent_model::{
  ModelError, ModelEvent, ModelProvider, ModelRegistry, ModelRequest, ModelStream,
};
use async_trait::async_trait;
use futures::stream;
use tokio::sync::Notify;

struct ReleasedProvider {
  entered: Arc<Notify>,
  release: Arc<Notify>,
}

#[async_trait]
impl ModelProvider for ReleasedProvider {
  fn provider_id(&self) -> &'static str {
    "released"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Ok(vec![ModelCapabilities {
      provider: "released".into(),
      model: "released-model".into(),
      tier: ModelTier::Worker,
      locality: Locality::Local,
      context_window: 4096,
      max_output_tokens: 1024,
      supports_tools: false,
      supports_streaming: true,
      supports_json: false,
      supports_thinking: false,
      cost_per_mtok_in: 0.0,
      cost_per_mtok_out: 0.0,
    }])
  }

  async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
    self.entered.notify_one();
    self.release.notified().await;
    Ok(ModelStream {
      model: "released-model".into(),
      events: Box::pin(stream::iter(vec![
        Ok(ModelEvent::Delta("done".into())),
        Ok(ModelEvent::Usage(TokenUsage::default())),
        Ok(ModelEvent::Done {
          stop_reason: StopReason::EndTurn,
        }),
      ])),
    })
  }
}

#[tokio::test]
async fn command_flood_does_not_starve_normal_turn_completion() {
  let entered = Arc::new(Notify::new());
  let release = Arc::new(Notify::new());
  let providers = ModelRegistry::new();
  providers.register(Arc::new(ReleasedProvider {
    entered: entered.clone(),
    release: release.clone(),
  }));
  let (controller, task) = spawn_runtime(Kernel::builder().providers(providers).build(), 4);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "finish under traffic".into(),
    })
    .await
    .unwrap();
  entered.notified().await;

  let stop = Arc::new(AtomicBool::new(false));
  let mut senders = Vec::new();
  for _ in 0..8 {
    let controller = controller.clone();
    let stop = stop.clone();
    senders.push(tokio::spawn(async move {
      while !stop.load(Ordering::Acquire) {
        if controller
          .send(AgentCommand::SelectSessionModel {
            selection: ModelSelection::Auto,
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
  release.notify_one();

  let completed = tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if snapshots
        .borrow()
        .turns
        .last()
        .is_some_and(|turn| turn.status == TurnStatus::Completed)
      {
        break;
      }
      snapshots.changed().await.unwrap();
    }
  })
  .await;
  stop.store(true, Ordering::Release);
  for sender in senders {
    sender.abort();
    let _ = sender.await;
  }

  if completed.is_err() {
    task.abort();
    let _ = task.join().await;
    panic!("steady command traffic starved normal turn completion");
  }
  assert_eq!(snapshots.borrow().phase, RuntimePhase::Completed);
  drop(controller);
  task.join().await.unwrap();
}

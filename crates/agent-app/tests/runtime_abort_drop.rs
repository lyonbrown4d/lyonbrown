use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use agent_app::{AgentCommand, RuntimePhase, spawn_runtime};
use agent_core::{Locality, ModelCapabilities, ModelTier};
use agent_kernel::Kernel;
use agent_model::{ModelError, ModelProvider, ModelRequest, ModelStream};
use async_trait::async_trait;
use futures::Stream;

struct DropProbe(Arc<AtomicBool>);

impl Drop for DropProbe {
  fn drop(&mut self) {
    self.0.store(true, Ordering::Release);
  }
}

struct PendingProbeStream {
  _probe: DropProbe,
}

impl Stream for PendingProbeStream {
  type Item = Result<agent_model::ModelEvent, ModelError>;

  fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    Poll::Pending
  }
}

struct ProbeProvider {
  dropped: Arc<AtomicBool>,
}

#[async_trait]
impl ModelProvider for ProbeProvider {
  fn provider_id(&self) -> &'static str {
    "probe"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Ok(vec![ModelCapabilities {
      provider: "probe".into(),
      model: "probe-model".into(),
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
    Ok(ModelStream {
      model: "probe-model".into(),
      events: Box::pin(PendingProbeStream {
        _probe: DropProbe(self.dropped.clone()),
      }),
    })
  }
}

#[tokio::test]
async fn aborting_runtime_drops_the_active_turn_instead_of_detaching_it() {
  let dropped = Arc::new(AtomicBool::new(false));
  let providers = agent_model::ModelRegistry::new();
  providers.register(Arc::new(ProbeProvider {
    dropped: dropped.clone(),
  }));
  let (controller, task) = spawn_runtime(Kernel::builder().providers(providers).build(), 4);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "wait".into(),
    })
    .await
    .unwrap();
  tokio::time::timeout(Duration::from_secs(2), async {
    while snapshots.borrow().phase != RuntimePhase::Streaming {
      snapshots.changed().await.unwrap();
    }
  })
  .await
  .expect("turn did not start");

  task.abort();
  assert!(task.join().await.is_err());
  tokio::time::timeout(Duration::from_secs(2), async {
    while !dropped.load(Ordering::Acquire) {
      tokio::task::yield_now().await;
    }
  })
  .await
  .expect("active turn was detached instead of dropped");
}

#[tokio::test]
async fn dropping_runtime_task_aborts_the_actor_instead_of_detaching_it() {
  let dropped = Arc::new(AtomicBool::new(false));
  let providers = agent_model::ModelRegistry::new();
  providers.register(Arc::new(ProbeProvider {
    dropped: dropped.clone(),
  }));
  let (controller, task) = spawn_runtime(Kernel::builder().providers(providers).build(), 4);
  let mut snapshots = controller.subscribe();
  controller
    .execute(AgentCommand::StartTurn {
      text: "wait".into(),
    })
    .await
    .unwrap();
  tokio::time::timeout(Duration::from_secs(2), async {
    while snapshots.borrow().phase != RuntimePhase::Streaming {
      snapshots.changed().await.unwrap();
    }
  })
  .await
  .unwrap();

  drop(task);

  tokio::time::timeout(Duration::from_secs(2), async {
    while !dropped.load(Ordering::Acquire) {
      tokio::task::yield_now().await;
    }
  })
  .await
  .expect("dropped RuntimeTask must not detach the actor");
}

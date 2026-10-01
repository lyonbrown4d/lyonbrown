use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use agent_core::{AgentEvent, Locality, ModelCapabilities, ModelTier, StopReason, TokenUsage};
use agent_kernel::Kernel;
use agent_model::{
  ModelError, ModelEvent, ModelProvider, ModelRegistry, ModelRequest, ModelStream,
};
use async_trait::async_trait;
use futures::stream;
use provider_mock::{MockProvider, ScriptedStep};
use tokio::sync::Notify;

struct PausedFirstProvider {
  calls: AtomicUsize,
  requests: Mutex<Vec<ModelRequest>>,
  release_first: Notify,
}

impl PausedFirstProvider {
  fn new() -> Self {
    Self {
      calls: AtomicUsize::new(0),
      requests: Mutex::new(Vec::new()),
      release_first: Notify::new(),
    }
  }

  fn request_count(&self) -> usize {
    self.calls.load(Ordering::Acquire)
  }

  fn release(&self) {
    self.release_first.notify_one();
  }

  async fn wait_until_called(&self) {
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
      while self.request_count() == 0 {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
  }
}

#[async_trait]
impl ModelProvider for PausedFirstProvider {
  fn provider_id(&self) -> &'static str {
    "paused"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Ok(vec![ModelCapabilities {
      provider: "paused".into(),
      model: "test-model".into(),
      tier: ModelTier::Worker,
      locality: Locality::Local,
      context_window: 32_768,
      max_output_tokens: 4_096,
      supports_tools: true,
      supports_streaming: true,
      supports_json: true,
      supports_thinking: false,
      cost_per_mtok_in: 0.0,
      cost_per_mtok_out: 0.0,
    }])
  }

  async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
    self
      .requests
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .push(request);
    let call = self.calls.fetch_add(1, Ordering::AcqRel);
    if call == 0 {
      self.release_first.notified().await;
    }
    let text = if call == 0 {
      "obsolete answer"
    } else {
      "steered answer"
    };
    Ok(ModelStream {
      model: "test-model".into(),
      events: Box::pin(stream::iter(vec![
        Ok(ModelEvent::Delta(text.into())),
        Ok(ModelEvent::Usage(TokenUsage::default())),
        Ok(ModelEvent::Done {
          stop_reason: StopReason::EndTurn,
        }),
      ])),
    })
  }
}

#[tokio::test]
async fn steering_during_model_work_restarts_at_the_next_safe_boundary() {
  let provider = Arc::new(PausedFirstProvider::new());
  let providers = ModelRegistry::new();
  providers.register(provider.clone());
  let mut kernel = Kernel::builder().providers(providers).build();
  let control = kernel.control();

  let task = tokio::spawn(async move {
    let result = kernel.run_turn("inspect the workspace").await;
    (kernel, result)
  });
  tokio::time::timeout(std::time::Duration::from_secs(1), async {
    while provider.request_count() == 0 {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();

  control.steer("focus on the Windows linker error").unwrap();
  provider.release();

  let (kernel, result) = tokio::time::timeout(std::time::Duration::from_secs(1), task)
    .await
    .unwrap()
    .unwrap();
  let events = result.unwrap();

  assert_eq!(provider.request_count(), 2);
  assert_eq!(
    events
      .iter()
      .filter(|event| matches!(event, AgentEvent::TurnSteered { .. }))
      .count(),
    1
  );
  assert_eq!(
    events.iter().find_map(AgentEvent::turn_answer),
    Some("steered answer")
  );
  assert!(kernel.event_log().iter().any(|event| matches!(
    event,
    AgentEvent::TurnSteered { text, .. } if text == "focus on the Windows linker error"
  )));
}

#[test]
fn steering_rejects_blank_input() {
  let kernel = Kernel::builder().build();
  let error = kernel.control().steer("   ").unwrap_err();

  assert_eq!(error.to_string(), "steering text must not be empty");
}

#[test]
fn steering_rejects_input_when_no_turn_is_running() {
  let kernel = Kernel::builder().build();
  let error = kernel.control().steer("do this later").unwrap_err();

  assert_eq!(error.to_string(), "turn is not accepting steering input");
}

#[tokio::test]
async fn a_new_turn_can_run_after_the_previous_turn_was_cancelled() {
  let providers = ModelRegistry::new();
  providers.register(Arc::new(MockProvider::scripted(vec![
    ScriptedStep::pending(),
    ScriptedStep::text("recovered"),
  ])));
  let mut kernel = Kernel::builder().providers(providers).build();
  let cancellation = kernel.cancellation_token();
  tokio::spawn(async move {
    tokio::task::yield_now().await;
    cancellation.cancel();
  });

  assert!(matches!(
    kernel.run_turn("cancel me").await,
    Err(agent_core::AgentError::Cancelled)
  ));
  let events = kernel.run_turn("try again").await.unwrap();

  assert_eq!(
    events.iter().find_map(AgentEvent::turn_answer),
    Some("recovered")
  );
}

#[tokio::test]
async fn steering_from_a_cancelled_turn_never_leaks_into_the_next_turn() {
  let provider = Arc::new(PausedFirstProvider::new());
  let providers = ModelRegistry::new();
  providers.register(provider.clone());
  let kernel = Kernel::builder().providers(providers).build();
  let control = kernel.control();
  let cancellation = kernel.cancellation_token();
  let task = tokio::spawn(async move {
    let mut kernel = kernel;
    let first = kernel.run_turn("first").await;
    (kernel, first)
  });
  provider.wait_until_called().await;
  control.steer("stale guidance").unwrap();
  cancellation.cancel();
  let (mut kernel, first) = task.await.unwrap();
  assert!(matches!(first, Err(agent_core::AgentError::Cancelled)));

  let events = kernel.run_turn("second").await.unwrap();

  assert!(
    !events
      .iter()
      .any(|event| matches!(event, AgentEvent::TurnSteered { .. }))
  );
}

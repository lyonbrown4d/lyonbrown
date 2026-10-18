use agent_inference::*;
use std::sync::Arc;
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;
struct HeldBackend {
  started: Arc<Notify>,
}
#[async_trait::async_trait]
impl InferenceBackend for HeldBackend {
  fn info(&self) -> ModelInfo {
    ModelInfo {
      id: "loaded".into(),
      backend: "fixture".into(),
      context_window: 4096,
      supports_tools: false,
      supports_images: false,
    }
  }
  async fn generate(
    &self,
    _: GenerationRequest,
    cancel: CancellationToken,
    _: mpsc::Sender<InferenceChunk>,
  ) -> Result<GenerationSummary> {
    self.started.notify_one();
    cancel.cancelled().await;
    Err(InferenceError::Cancelled)
  }
}
fn request() -> GenerationRequest {
  GenerationRequest {
    messages: vec![ChatMessage {
      role: MessageRole::User,
      content: vec![InputContent::Text {
        text: "hello".into(),
      }],
      tool_calls: Vec::new(),
      tool_call_id: None,
    }],
    tools: Vec::new(),
    max_tokens: 4,
    temperature: None,
    seed: 0,
  }
}
#[tokio::test]
async fn unavailable_capability_and_cancelled_waiter_do_not_start_generation() {
  let registry = Arc::new(ModelRegistry::new(1).unwrap());
  assert!(registry.models().await.is_empty());
  assert!(matches!(
    registry.backend("loaded").await,
    Err(InferenceError::ModelUnavailable(_))
  ));
  let started = Arc::new(Notify::new());
  registry
    .register(
      Arc::new(HeldBackend {
        started: started.clone(),
      }),
      1,
    )
    .await
    .unwrap();
  let backend = registry.backend("loaded").await.unwrap();
  let (sender, _receiver) = mpsc::channel(1);
  let cancel = CancellationToken::new();
  let first = {
    let backend = backend.clone();
    let cancel = cancel.clone();
    let sender = sender.clone();
    tokio::spawn(async move { backend.generate(request(), cancel, sender).await })
  };
  started.notified().await;
  let waiting = CancellationToken::new();
  waiting.cancel();
  assert!(matches!(
    backend.generate(request(), waiting, sender.clone()).await,
    Err(InferenceError::Cancelled)
  ));
  let mut image = request();
  image.messages[0].content.push(InputContent::Image {
    mime: "image/png".into(),
    data_base64: "unread".into(),
  });
  assert!(matches!(
    backend
      .generate(image, CancellationToken::new(), sender)
      .await,
    Err(InferenceError::InvalidRequest(_))
  ));
  cancel.cancel();
  assert!(matches!(
    first.await.unwrap(),
    Err(InferenceError::Cancelled)
  ));
  assert!(registry.remove("loaded").await);
  assert!(registry.models().await.is_empty());
}

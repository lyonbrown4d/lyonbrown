use agent_inference::*;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

pub struct Backend {
  info: ModelInfo,
  reply: Option<String>,
  pub started: tokio::sync::Notify,
  pub requests: Mutex<Vec<GenerationRequest>>,
}
impl Backend {
  pub fn new(id: &str, tools: bool, reply: Option<&str>) -> Arc<Self> {
    Arc::new(Self {
      info: ModelInfo {
        id: id.into(),
        backend: "synthetic".into(),
        context_window: 4096,
        supports_tools: tools,
        supports_images: false,
      },
      reply: reply.map(str::to_owned),
      started: tokio::sync::Notify::new(),
      requests: Mutex::new(Vec::new()),
    })
  }
}
#[async_trait::async_trait]
impl InferenceBackend for Backend {
  fn info(&self) -> ModelInfo {
    self.info.clone()
  }
  async fn generate(
    &self,
    request: GenerationRequest,
    cancellation: CancellationToken,
    output: mpsc::Sender<InferenceChunk>,
  ) -> agent_inference::Result<GenerationSummary> {
    if cancellation.is_cancelled() {
      return Err(InferenceError::Cancelled);
    }
    self.requests.lock().await.push(request);
    self.started.notify_one();
    let Some(reply) = &self.reply else {
      cancellation.cancelled().await;
      return Err(InferenceError::Cancelled);
    };
    output
      .send(InferenceChunk::Text {
        text: reply.clone(),
      })
      .await
      .map_err(|_| InferenceError::ConsumerDisconnected)?;
    Ok(GenerationSummary {
      reason: FinishReason::Stop,
      input_tokens: 2,
      output_tokens: 2,
    })
  }
}

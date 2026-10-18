use agent_decision::{DecisionEngine, DecisionPolicy, LocalClassifier};
use agent_inference::{
  FinishReason, GenerationRequest, GenerationSummary, InferenceBackend, InferenceChunk,
  InferenceError, ModelInfo,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy)]
pub enum Completion {
  Finish(FinishReason),
  Error,
  Stall { close_output: bool },
}

pub struct FixtureBackend {
  chunks: Vec<InferenceChunk>,
  completion: Completion,
  pub context_window: u32,
  pub request: Mutex<Option<GenerationRequest>>,
  cancellation: Mutex<Option<CancellationToken>>,
  pub started: Notify,
}
impl FixtureBackend {
  pub fn new(chunks: Vec<InferenceChunk>, completion: Completion) -> Self {
    Self {
      chunks,
      completion,
      context_window: 16_384,
      request: Mutex::new(None),
      cancellation: Mutex::new(None),
      started: Notify::new(),
    }
  }
  pub fn token(&self) -> CancellationToken {
    self.cancellation.lock().unwrap().as_ref().unwrap().clone()
  }
}
#[async_trait]
impl InferenceBackend for FixtureBackend {
  fn info(&self) -> ModelInfo {
    ModelInfo {
      id: "synthetic-local-classifier".into(),
      backend: "test".into(),
      context_window: self.context_window,
      supports_tools: true,
      supports_images: false,
    }
  }
  async fn generate(
    &self,
    request: GenerationRequest,
    cancellation: CancellationToken,
    output: mpsc::Sender<InferenceChunk>,
  ) -> agent_inference::Result<GenerationSummary> {
    *self.request.lock().unwrap() = Some(request);
    *self.cancellation.lock().unwrap() = Some(cancellation.clone());
    self.started.notify_one();
    for chunk in self.chunks.clone() {
      output
        .send(chunk)
        .await
        .map_err(|_| InferenceError::ConsumerDisconnected)?;
    }
    match self.completion {
      Completion::Finish(reason) => {
        // Closing the channel before returning exercises both completion orders.
        drop(output);
        tokio::task::yield_now().await;
        Ok(GenerationSummary {
          reason,
          input_tokens: 1,
          output_tokens: 1,
        })
      }
      Completion::Error => Err(InferenceError::Backend("synthetic private detail".into())),
      Completion::Stall { close_output } => {
        let retained = (!close_output).then_some(output);
        cancellation.cancelled().await;
        drop(retained);
        Err(InferenceError::Cancelled)
      }
    }
  }
}

pub fn local_engine(backend: Arc<FixtureBackend>, policy: DecisionPolicy) -> Arc<DecisionEngine> {
  Arc::new(DecisionEngine::new(policy, Some(Arc::new(LocalClassifier::new(backend)))).unwrap())
}

//! Synthetic in-process inference through the real public backend boundary.
//! This loads no model weights and does not exercise llama.cpp or mistral.rs SDKs.
use agent_inference::{
  FinishReason, GenerationRequest, GenerationSummary, InferenceBackend, InferenceChunk,
  InferenceError, ModelInfo, Result,
};
use std::{
  collections::VecDeque,
  sync::atomic::{AtomicU64, Ordering},
  time::Duration,
};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub enum MockInferenceStep {
  Chunk(InferenceChunk),
  Delay(Duration),
  Error(String),
  Stall,
}
#[derive(Clone)]
pub struct MockGeneration {
  pub steps: Vec<MockInferenceStep>,
  pub summary: GenerationSummary,
}
impl MockGeneration {
  pub fn text(text: impl Into<String>) -> Self {
    Self {
      steps: vec![MockInferenceStep::Chunk(InferenceChunk::Text {
        text: text.into(),
      })],
      summary: GenerationSummary {
        reason: FinishReason::Stop,
        input_tokens: 1,
        output_tokens: 1,
      },
    }
  }
  pub fn stalled() -> Self {
    Self {
      steps: vec![MockInferenceStep::Stall],
      ..Self::text("")
    }
  }
}
#[derive(Default)]
struct Counters {
  requests: AtomicU64,
  completed: AtomicU64,
  cancelled: AtomicU64,
  failed: AtomicU64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MockInferenceCounts {
  pub requests: u64,
  pub completed: u64,
  pub cancelled: u64,
  pub failed: u64,
}
pub struct MockInferenceBackend {
  info: ModelInfo,
  generations: Mutex<VecDeque<MockGeneration>>,
  counters: Counters,
}
impl MockInferenceBackend {
  pub fn new(mut info: ModelInfo, generations: Vec<MockGeneration>) -> Result<Self> {
    if info.id.is_empty()
      || info.context_window < 2
      || generations.is_empty()
      || generations.len() > 128
      || generations
        .iter()
        .any(|generation| generation.steps.len() > 128)
    {
      return Err(InferenceError::InvalidRequest(
        "invalid bounded synthetic inference script".into(),
      ));
    }
    info.backend = "synthetic/model-mock".into();
    Ok(Self {
      info,
      generations: Mutex::new(generations.into()),
      counters: Counters::default(),
    })
  }
  pub fn counts(&self) -> MockInferenceCounts {
    MockInferenceCounts {
      requests: self.counters.requests.load(Ordering::Relaxed),
      completed: self.counters.completed.load(Ordering::Relaxed),
      cancelled: self.counters.cancelled.load(Ordering::Relaxed),
      failed: self.counters.failed.load(Ordering::Relaxed),
    }
  }
  pub fn calls(&self) -> u64 {
    self.counts().requests
  }
}
fn increment(counter: &AtomicU64) {
  let _ = counter.try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
    Some(count.saturating_add(1))
  });
}
// A dropped generate future is cancellation, including timeout in its caller.
struct Attempt<'a> {
  counters: &'a Counters,
  finished: bool,
}
impl Drop for Attempt<'_> {
  fn drop(&mut self) {
    if !self.finished {
      increment(&self.counters.cancelled);
    }
  }
}
#[async_trait::async_trait]
impl InferenceBackend for MockInferenceBackend {
  fn info(&self) -> ModelInfo {
    self.info.clone()
  }
  async fn generate(
    &self,
    _request: GenerationRequest,
    cancellation: CancellationToken,
    output: mpsc::Sender<InferenceChunk>,
  ) -> Result<GenerationSummary> {
    if cancellation.is_cancelled() {
      return Err(InferenceError::Cancelled);
    }
    increment(&self.counters.requests);
    let mut attempt = Attempt {
      counters: &self.counters,
      finished: false,
    };
    let script = self.generations.lock().await.pop_front();
    let result = async {
      let generation =
        script.ok_or_else(|| InferenceError::Backend("synthetic script exhausted".into()))?;
      for step in generation.steps {
        match step {
          MockInferenceStep::Chunk(chunk) => tokio::select! {
            biased;
            _=cancellation.cancelled()=>return Err(InferenceError::Cancelled),
            result=output.send(chunk)=>result.map_err(|_| InferenceError::ConsumerDisconnected)?,
          },
          MockInferenceStep::Delay(duration) => tokio::select! {
            biased;
            _=cancellation.cancelled()=>return Err(InferenceError::Cancelled),
            _=tokio::time::sleep(duration)=>{},
          },
          MockInferenceStep::Error(message) => return Err(InferenceError::Backend(message)),
          MockInferenceStep::Stall => {
            cancellation.cancelled().await;
            return Err(InferenceError::Cancelled);
          }
        }
      }
      if cancellation.is_cancelled() {
        return Err(InferenceError::Cancelled);
      }
      Ok(generation.summary)
    }
    .await;
    attempt.finished = true;
    match &result {
      Ok(_) => increment(&self.counters.completed),
      Err(InferenceError::Cancelled) => increment(&self.counters.cancelled),
      Err(_) => increment(&self.counters.failed),
    }
    result
  }
}

//! Explicit opt-in acceptance against models already present on this machine.
#![cfg(any(feature = "llama", feature = "mistral"))]
use agent_inference::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
fn request() -> GenerationRequest {
  GenerationRequest {
    messages: vec![ChatMessage {
      role: MessageRole::User,
      content: vec![InputContent::Text {
        text: "Say hello briefly.".into(),
      }],
      tool_calls: Vec::new(),
      tool_call_id: None,
    }],
    tools: Vec::new(),
    max_tokens: 16,
    temperature: Some(0.0),
    seed: 0,
  }
}
async fn generate(
  backend: &dyn InferenceBackend,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
  let (sender, mut receiver) = mpsc::channel(64);
  let result = backend
    .generate(request(), CancellationToken::new(), sender)
    .await?;
  let mut text = String::new();
  while let Some(chunk) = receiver.recv().await {
    if let InferenceChunk::Text { text: part } = chunk {
      text.push_str(&part);
    }
  }
  assert!(!text.trim().is_empty(), "the real model must produce text");
  assert!(matches!(
    result.reason,
    FinishReason::Stop | FinishReason::Length
  ));
  Ok(())
}
#[cfg(feature = "llama")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires LYONBROWN_TEST_GGUF pointing to an existing chat GGUF"]
async fn llama_real_model_generates() -> std::result::Result<(), Box<dyn std::error::Error>> {
  let path = std::env::var("LYONBROWN_TEST_GGUF")?;
  let backend = LlamaCppBackend::load("acceptance-llama".into(), path, 2048, 0).await?;
  generate(&backend).await
}
#[cfg(feature = "mistral")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires LYONBROWN_TEST_GGUF pointing to an existing chat GGUF"]
async fn mistral_real_model_generates() -> std::result::Result<(), Box<dyn std::error::Error>> {
  let path = std::env::var("LYONBROWN_TEST_GGUF")?;
  let backend =
    MistralBackend::load_gguf("acceptance-mistral".into(), path, 2048, false, true).await?;
  generate(&backend).await
}

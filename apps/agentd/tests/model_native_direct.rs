//! Diagnostic real-weight generation without Harness/session restoration.
#![cfg(feature = "local-llama")]
use agent_inference::{
  ChatMessage, GenerationRequest, InferenceBackend, InferenceChunk, InputContent, LlamaCppBackend,
  MessageRole,
};
use anyhow::{Result, ensure};
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "uses cached SmolLM2 real GGUF to distinguish model behavior from Harness history"]
async fn direct_real_model_distinguishes_independent_user_prompts() -> Result<()> {
  let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/model-acceptance/models/models/c385da64aabdd20916d81b9a78f38f14e8830fc089e68158b9251c1be7dac991/SmolLM2-135M-Instruct.Q4_K_M.gguf");
  let backend = LlamaCppBackend::load("direct-diagnostic".into(), path, 4096, 0).await?;
  let mut answers = Vec::new();
  for prompt in [
    "Say hello briefly in one sentence.",
    "Repeat this word only: violet.",
    "What is 2 + 2? Answer with one number.",
  ] {
    let (send, mut receive) = tokio::sync::mpsc::channel(64);
    let messages = vec![
      ChatMessage {
        role: MessageRole::System,
        content: vec![InputContent::Text {
          text: "You are a helpful assistant. Follow the user's instruction precisely.".into(),
        }],
        tool_calls: vec![],
        tool_call_id: None,
      },
      ChatMessage {
        role: MessageRole::User,
        content: vec![InputContent::Text {
          text: prompt.into(),
        }],
        tool_calls: vec![],
        tool_call_id: None,
      },
    ];
    let summary = backend
      .generate(
        GenerationRequest {
          messages,
          tools: vec![],
          max_tokens: 32,
          temperature: Some(0.0),
          seed: 0,
        },
        CancellationToken::new(),
        send,
      )
      .await?;
    let mut answer = String::new();
    while let Some(chunk) = receive.recv().await {
      if let InferenceChunk::Text { text } = chunk {
        answer.push_str(&text);
      }
    }
    println!(
      "DIRECT_MODEL prompt={prompt:?} response={answer:?} tokens={}",
      summary.output_tokens
    );
    answers.push(answer);
  }
  ensure!(
    answers[1].to_ascii_lowercase().contains("violet"),
    "direct model did not follow marker instruction"
  );
  ensure!(
    answers[2].contains('4'),
    "direct model did not answer independent arithmetic prompt"
  );
  Ok(())
}

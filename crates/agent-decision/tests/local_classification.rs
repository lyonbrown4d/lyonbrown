mod local_support;
#[allow(dead_code)]
mod support;

use agent_decision::*;
use agent_inference::{FinishReason, InferenceChunk, InputContent, MessageRole, ToolCall};
use local_support::*;
use std::{sync::Arc, time::Duration};
use support::*;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn local_stream_routes_structured_output_with_no_tools_and_bounded_request() {
  let output = candidate("code", &["write_file"]).to_string();
  // More than channel capacity verifies generation and consumption progress together.
  let chunks = output
    .chars()
    .map(|character| InferenceChunk::Text {
      text: character.to_string(),
    })
    .collect();
  let backend = Arc::new(FixtureBackend::new(
    chunks,
    Completion::Finish(FinishReason::Stop),
  ));
  let plan = local_engine(backend.clone(), policy())
    .decide("synthetic local request", None, CancellationToken::new())
    .await
    .unwrap();
  assert_eq!(plan.task_kind, TaskKind::Code);
  assert_eq!(plan.allowed_tools, tools(&[Tool::WriteFile]));
  assert_eq!(plan.risk_level, RiskLevel::High);
  assert_eq!(
    plan.source,
    DecisionSource::Classifier { restricted: false }
  );
  let request = backend.request.lock().unwrap();
  let request = request.as_ref().unwrap();
  assert!(request.tools.is_empty());
  assert_eq!(request.max_tokens, 2048);
  assert_eq!(request.temperature, Some(0.0));
  assert_eq!(request.messages[0].role, MessageRole::System);
  assert_eq!(request.messages[1].role, MessageRole::User);
  assert!(
    matches!(&request.messages[1].content[0], InputContent::Text { text } if text == "synthetic local request")
  );
  assert!(
    request
      .messages
      .iter()
      .all(|message| message.tool_calls.is_empty())
  );
  assert!(
    backend.token().is_cancelled(),
    "generation resources released after success"
  );
}

#[tokio::test]
async fn local_output_limit_counts_text_and_thinking_bytes() {
  let output = candidate("chat", &[]).to_string();
  let limit = agent_runtime::CLASSIFICATION_OUTPUT_LIMIT;
  for (thinking, text, accepted) in [
    ("x".repeat(limit - output.len()), output.clone(), true),
    ("x".repeat(limit - output.len() + 1), output.clone(), false),
    (
      String::new(),
      format!("{}{}", " ".repeat(limit - output.len()), output),
      true,
    ),
    // UTF-8 byte length matters for both text and thinking streams.
    (String::new(), "界".repeat(limit / 3 + 1), false),
  ] {
    let backend = Arc::new(FixtureBackend::new(
      vec![
        InferenceChunk::Thinking { text: thinking },
        InferenceChunk::Text { text },
      ],
      Completion::Finish(FinishReason::Stop),
    ));
    let plan = local_engine(backend.clone(), policy())
      .decide("fixture", None, CancellationToken::new())
      .await
      .unwrap();
    if accepted {
      assert_eq!(plan.task_kind, TaskKind::Chat);
      assert_eq!(
        plan.source,
        DecisionSource::Classifier { restricted: false }
      );
    } else {
      assert_fallback(&plan, FallbackReason::Unavailable);
    }
    assert!(backend.token().is_cancelled());
  }
}

#[tokio::test]
async fn local_tool_output_incomplete_output_and_backend_errors_are_tool_free() {
  let text = InferenceChunk::Text {
    text: candidate("code", &["write_file"]).to_string(),
  };
  for (chunks, completion) in [
    (vec![text.clone()], Completion::Finish(FinishReason::Length)),
    (
      vec![text.clone()],
      Completion::Finish(FinishReason::ToolCalls),
    ),
    (vec![text.clone()], Completion::Error),
    (vec![], Completion::Finish(FinishReason::Stop)),
    (
      vec![InferenceChunk::Thinking {
        text: "private reasoning".into(),
      }],
      Completion::Finish(FinishReason::Stop),
    ),
    (
      vec![
        text,
        InferenceChunk::ToolCall {
          call: ToolCall {
            id: "forbidden".into(),
            name: "write_file".into(),
            arguments: serde_json::json!({"path":"must-not-write"}),
          },
        },
      ],
      Completion::Finish(FinishReason::Stop),
    ),
  ] {
    let backend = Arc::new(FixtureBackend::new(chunks, completion));
    let plan = local_engine(backend.clone(), policy())
      .decide("fixture", None, CancellationToken::new())
      .await
      .unwrap();
    assert_fallback(&plan, FallbackReason::Unavailable);
    assert!(backend.token().is_cancelled());
  }
}

#[tokio::test]
async fn closed_local_stream_still_obeys_deadline_and_caller_cancellation() {
  for close_output in [false, true] {
    let backend = Arc::new(FixtureBackend::new(
      vec![],
      Completion::Stall { close_output },
    ));
    let mut bounded = policy();
    bounded.classifier_timeout = Duration::from_millis(20);
    let plan = local_engine(backend.clone(), bounded)
      .decide("fixture", None, CancellationToken::new())
      .await
      .unwrap();
    assert_fallback(&plan, FallbackReason::Timeout);
    assert!(
      backend.token().is_cancelled(),
      "deadline must cancel inference"
    );

    let backend = Arc::new(FixtureBackend::new(
      vec![],
      Completion::Stall { close_output },
    ));
    let engine = local_engine(backend.clone(), policy());
    let cancellation = CancellationToken::new();
    let stop = cancellation.clone();
    let task = tokio::spawn(async move { engine.decide("fixture", None, stop).await });
    tokio::time::timeout(Duration::from_secs(1), backend.started.notified())
      .await
      .unwrap();
    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), task)
      .await
      .unwrap()
      .unwrap();
    assert_eq!(result.unwrap_err(), DecisionError::Cancelled);
    assert!(
      backend.token().is_cancelled(),
      "caller cancellation must reach inference"
    );
  }
}

#[tokio::test]
async fn invalid_context_and_already_cancelled_requests_never_generate() {
  let mut backend = FixtureBackend::new(vec![], Completion::Finish(FinishReason::Stop));
  backend.context_window = 1;
  let backend = Arc::new(backend);
  let plan = local_engine(backend.clone(), policy())
    .decide("fixture", None, CancellationToken::new())
    .await
    .unwrap();
  assert_fallback(&plan, FallbackReason::Unavailable);
  assert!(backend.request.lock().unwrap().is_none());
  let backend = Arc::new(FixtureBackend::new(
    vec![],
    Completion::Finish(FinishReason::Stop),
  ));
  let cancellation = CancellationToken::new();
  cancellation.cancel();
  assert_eq!(
    local_engine(backend.clone(), policy())
      .decide("fixture", None, cancellation)
      .await
      .unwrap_err(),
    DecisionError::Cancelled
  );
  assert!(backend.request.lock().unwrap().is_none());
}

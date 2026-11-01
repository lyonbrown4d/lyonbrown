use std::time::Duration;

use futures::{StreamExt, stream};
use mistralrs::{
  CalledFunction, ChatCompletionChunkResponse, ChunkChoice, Delta, Response, ToolCallResponse,
  ToolCallType,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{FinishReason, InferenceChunk, InferenceError};

fn terminal(index: usize) -> Response {
  let call = ToolCallResponse {
    index: 0,
    id: "call-1".into(),
    tp: ToolCallType::Function,
    function: CalledFunction {
      name: "read_file".into(),
      arguments: "{\"path\":\"marker.txt\"}".into(),
    },
  };
  Response::Chunk(ChatCompletionChunkResponse {
    id: "response-1".into(),
    choices: vec![ChunkChoice {
      delta: Delta {
        content: None,
        role: "assistant".into(),
        tool_calls: Some(vec![call]),
        reasoning_content: None,
      },
      index,
      finish_reason: Some("tool_calls".into()),
      logprobs: None,
    }],
    created: 0,
    model: "native-test".into(),
    system_fingerprint: String::new(),
    object: "chat.completion.chunk".into(),
    usage: None,
  })
}

#[tokio::test]
async fn terminal_tool_choice_is_emitted_without_waiting_for_sender_close() {
  let stream = stream::iter([terminal(0)]).chain(stream::pending());
  let (sender, mut receiver) = mpsc::channel(4);
  let summary = tokio::time::timeout(
    Duration::from_secs(1),
    crate::mistral_stream::collect(stream, CancellationToken::new(), sender),
  )
  .await
  .expect("terminal response must finish while SDK stream remains open")
  .expect("valid response");
  assert_eq!(summary.reason, FinishReason::ToolCalls);
  let Some(InferenceChunk::ToolCall { call }) = receiver.recv().await else {
    panic!("tool call required");
  };
  assert_eq!(call.id, "call-1");
  assert_eq!(call.name, "read_file");
  assert_eq!(call.arguments["path"], "marker.txt");
}

#[tokio::test]
async fn cancellation_and_invalid_choice_never_emit_tools() {
  let (sender, mut receiver) = mpsc::channel(4);
  let cancellation = CancellationToken::new();
  cancellation.cancel();
  let result =
    crate::mistral_stream::collect(stream::iter([terminal(0)]), cancellation, sender).await;
  assert!(matches!(result, Err(InferenceError::Cancelled)));
  assert!(receiver.recv().await.is_none());

  let (sender, mut receiver) = mpsc::channel(4);
  assert!(
    crate::mistral_stream::collect(
      stream::iter([terminal(1)]),
      CancellationToken::new(),
      sender
    )
    .await
    .is_err()
  );
  assert!(receiver.recv().await.is_none());
}

#[tokio::test]
async fn stream_close_without_terminal_is_an_error() {
  let (sender, _) = mpsc::channel(4);
  assert!(
    crate::mistral_stream::collect(stream::empty(), CancellationToken::new(), sender)
      .await
      .is_err()
  );
}

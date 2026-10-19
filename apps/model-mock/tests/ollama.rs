mod support;
use futures::StreamExt;
use model_mock::{MockApp, Protocol, Reply};
use ollama_rs::{
  Ollama,
  generation::{
    chat::{ChatMessage, request::ChatMessageRequest},
    completion::request::GenerationRequest,
  },
};
use serde_json::{Value, json};
use std::time::Duration;

fn client(app: &MockApp) -> Ollama {
  Ollama::try_new(app.base_url()).unwrap()
}
fn chat() -> ChatMessageRequest {
  ChatMessageRequest::new(
    "synthetic".into(),
    vec![ChatMessage::user("synthetic prompt".into())],
  )
}
#[tokio::test]
async fn published_ollama_client_parses_chat_json_ndjson_tools_and_metadata() {
  let app = support::app(
    Protocol::OllamaChat,
    vec![
      support::text("json chat"),
      support::text("stream chat"),
      Reply::Tool {
        name: "fixture_echo".into(),
        arguments: json!({"text":"synthetic"}),
        delay_ms: 0,
      },
      support::text("tool result accepted"),
    ],
  )
  .await;
  let client = client(&app);
  let tags = client.list_local_models().await.unwrap();
  assert_eq!(tags[0].name, "synthetic");
  assert_eq!(tags[0].size, 0);
  let info = client.show_model_info("synthetic".into()).await.unwrap();
  assert!(info.capabilities.contains(&"tools".into()));
  assert!(info.license.contains("Synthetic"));
  assert!(client.show_model_info("absent".into()).await.is_err());
  let full = client.send_chat_messages(chat()).await.unwrap();
  assert!(full.done);
  assert_eq!(full.message.content, "json chat");
  assert_eq!(full.final_data.unwrap().eval_count, 3);
  let mut stream = client.send_chat_messages_stream(chat()).await.unwrap();
  let mut text = String::new();
  let mut terminal = false;
  while let Some(response) = stream.next().await {
    let response = response.unwrap();
    text.push_str(&response.message.content);
    terminal |= response.done;
  }
  assert!(terminal);
  assert_eq!(text, "stream chat");
  let tool = client.send_chat_messages(chat()).await.unwrap();
  assert_eq!(tool.message.tool_calls[0].function.name, "fixture_echo");
  assert_eq!(
    tool.message.tool_calls[0].function.arguments,
    json!({"text":"synthetic"})
  );
  let final_response = client
    .send_chat_messages(ChatMessageRequest::new(
      "synthetic".into(),
      vec![tool.message, ChatMessage::tool("synthetic".into())],
    ))
    .await
    .unwrap();
  assert_eq!(final_response.message.content, "tool result accepted");
  let version: Value = reqwest::get(format!("{}/api/version", app.base_url()))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(version["version"], "0.0.0-synthetic");
}
#[tokio::test]
async fn published_ollama_client_parses_generate_and_reports_failure_without_false_completion() {
  let app = support::app(
    Protocol::OllamaGenerate,
    vec![
      support::text("json generate"),
      support::text("stream generate"),
      Reply::Truncated {
        text: "partial".into(),
      },
      Reply::Malformed {
        body: "{not json}\n".into(),
      },
      Reply::Error {
        status: 503,
        delay_ms: 0,
      },
    ],
  )
  .await;
  let client = client(&app);
  let request = || GenerationRequest::new("synthetic".into(), "synthetic prompt");
  let full = client.generate(request()).await.unwrap();
  assert!(full.done);
  assert_eq!(full.response, "json generate");
  assert_eq!(full.context.unwrap().0, vec![1, 2, 3]);
  let mut stream = client.generate_stream(request()).await.unwrap();
  let mut text = String::new();
  let mut terminal = false;
  while let Some(batch) = stream.next().await {
    for response in batch.unwrap() {
      text.push_str(&response.response);
      terminal |= response.done;
    }
  }
  assert!(terminal);
  assert_eq!(text, "stream generate");
  for _ in 0..2 {
    let mut stream = client.generate_stream(request()).await.unwrap();
    let mut terminal = false;
    while let Some(batch) = stream.next().await {
      for response in batch.unwrap() {
        terminal |= response.done;
      }
    }
    assert!(
      !terminal,
      "truncated/malformed transport cannot imply completion"
    );
  }
  assert!(client.generate(request()).await.is_err());
  assert!(
    client
      .generate(GenerationRequest::new("absent".into(), "prompt"))
      .await
      .is_err()
  );
}
#[tokio::test]
async fn missing_terminal_can_be_cancelled_and_releases_stream_concurrency() {
  let mut settings = support::settings(
    Protocol::OllamaChat,
    vec![
      Reply::MissingTerminal {
        text: "partial".into(),
      },
      support::text("after cancellation"),
    ],
  );
  settings.max_concurrency = 1;
  let app = MockApp::start(settings).await.unwrap();
  let client = client(&app);
  let mut stream = client.send_chat_messages_stream(chat()).await.unwrap();
  assert_eq!(
    stream.next().await.unwrap().unwrap().message.content,
    "partial"
  );
  assert!(
    tokio::time::timeout(Duration::from_millis(20), stream.next())
      .await
      .is_err()
  );
  let http = reqwest::Client::new();
  let url = format!("{}/api/chat", app.base_url());
  assert_eq!(
    http
      .post(&url)
      .json(&json!({"model":"synthetic"}))
      .send()
      .await
      .unwrap()
      .status(),
    429
  );
  drop(stream);
  let response = tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      let response = http
        .post(&url)
        .json(&json!({"model":"synthetic","stream":false}))
        .send()
        .await
        .unwrap();
      if response.status() != 429 {
        break response;
      }
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  let body: Value = response.json().await.unwrap();
  assert_eq!(body["message"]["content"], "after cancellation");
  app.shutdown().await;
}

mod support;
use futures::StreamExt;
use model_mock::{Protocol, Reply};
use rig_core::{
  completion::CompletionRequest,
  message::{AssistantContent, Message},
  streaming::{Item, StreamEvent},
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn actual_adapters_consume_fragmented_text_and_tool_result_roundtrips() {
  for protocol in support::PROTOCOLS {
    let app = support::app(
      protocol,
      vec![
        support::text("synthetic text"),
        Reply::Tool {
          name: "fixture_echo".into(),
          arguments: json!({"text":"synthetic"}),
          delay_ms: 0,
        },
        support::text("tool completed"),
      ],
    )
    .await;
    let config = support::config(&app, protocol);
    assert_eq!(
      model_provider::classify_json(&config, "synthetic prompt", CancellationToken::new())
        .await
        .unwrap(),
      "synthetic text"
    );
    let model = model_provider::configured(&config).unwrap();
    let prompt = Message::user("call fixture_echo");
    let mut stream = model
      .stream(CompletionRequest::new(prompt.clone()).max_tokens(128))
      .unwrap();
    let mut arguments = String::new();
    while let Some(item) = stream.next().await {
      if let Item::Event(StreamEvent::Arguments { json, .. }) = item.unwrap() {
        arguments.push_str(&json);
      }
    }
    let response = stream.finish().await.unwrap();
    let tool = response
      .choice
      .iter()
      .find_map(|content| match content {
        AssistantContent::ToolCall(tool) => Some(tool),
        _ => None,
      })
      .unwrap();
    assert_eq!(tool.function.name.as_str(), "fixture_echo");
    assert_eq!(tool.function.arguments, json!({"text":"synthetic"}));
    assert_eq!(
      serde_json::from_str::<Value>(&arguments).unwrap(),
      tool.function.arguments
    );
    let request = CompletionRequest::new(Message::tool_result(
      tool.id.clone(),
      tool.function.name.clone(),
      "synthetic",
    ))
    .messages([prompt, response.message().unwrap()])
    .max_tokens(128);
    let mut stream = model.stream(request).unwrap();
    let mut final_text = String::new();
    while let Some(item) = stream.next().await {
      if let Item::Event(StreamEvent::Text { text, .. }) = item.unwrap() {
        final_text.push_str(&text);
      }
    }
    let response = stream.finish().await.unwrap();
    assert_eq!(response.text(), "tool completed");
    assert_eq!(final_text, "tool completed");
    let health: Value = reqwest::get(format!("{}/health", app.base_url()))
      .await
      .unwrap()
      .json()
      .await
      .unwrap();
    assert_eq!(health["scenarios"][0]["requests"], 3);
    assert!(!health.to_string().contains("synthetic-secret"));
  }
}
#[tokio::test]
async fn finite_json_contracts_are_protocol_specific_and_exhaustion_is_explicit() {
  for protocol in support::PROTOCOLS {
    let app = support::app(protocol, vec![support::text("nonstream")]).await;
    let client = reqwest::Client::new();
    let url = format!("{}{}", app.base_url(), protocol.standard().unwrap().path());
    let response: Value = client
      .post(&url)
      .json(&json!({"model":"synthetic","stream":false}))
      .send()
      .await
      .unwrap()
      .json()
      .await
      .unwrap();
    let text = match protocol {
      Protocol::ChatCompletions => &response["choices"][0]["message"]["content"],
      Protocol::Responses => &response["output"][0]["content"][0]["text"],
      Protocol::Anthropic => &response["content"][0]["text"],
      _ => unreachable!(),
    };
    assert_eq!(text, "nonstream");
    assert_eq!(
      client
        .post(&url)
        .json(&json!({"model":"synthetic"}))
        .send()
        .await
        .unwrap()
        .status(),
      409
    );
    assert_eq!(
      client
        .post(&url)
        .json(&json!({"model":"absent"}))
        .send()
        .await
        .unwrap()
        .status(),
      404
    );
  }
}

#[tokio::test]
async fn delay_obeys_caller_timeout_and_cancelled_stream_releases_capacity() {
  for protocol in support::PROTOCOLS {
    let mut settings = support::settings(
      protocol,
      vec![
        Reply::Text {
          text: "must not finish early".into(),
          delay_ms: 10_000,
        },
        support::text("after cancellation"),
      ],
    );
    settings.max_concurrency = 1;
    let app = model_mock::MockApp::start(settings).await.unwrap();
    let config = support::config(&app, protocol);
    let stop = CancellationToken::new();
    let mut completion = {
      let config = config.clone();
      let stop = stop.clone();
      tokio::spawn(async move { model_provider::classify_json(&config, "synthetic", stop).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
      loop {
        let health: Value = reqwest::get(format!("{}/health", app.base_url()))
          .await
          .unwrap()
          .json()
          .await
          .unwrap();
        if health["scenarios"][0]["requests"] == 1 {
          break;
        }
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
    assert!(
      tokio::time::timeout(std::time::Duration::from_millis(20), &mut completion)
        .await
        .is_err()
    );
    stop.cancel();
    assert!(completion.await.unwrap().is_err());
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
      loop {
        if let Ok(text) =
          model_provider::classify_json(&config, "synthetic", CancellationToken::new()).await
        {
          break text;
        }
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
    assert_eq!(result, "after cancellation");
    app.shutdown().await;
  }
}
#[tokio::test]
async fn malformed_and_oversized_requests_do_not_advance_synthetic_scenario() {
  let app = support::app(Protocol::ChatCompletions, vec![support::text("intact")]).await;
  let client = reqwest::Client::new();
  let url = format!("{}/v1/chat/completions", app.base_url());
  assert_eq!(
    client
      .post(&url)
      .body("not JSON")
      .send()
      .await
      .unwrap()
      .status(),
    400
  );
  assert_eq!(
    client
      .post(&url)
      .body(vec![b'x'; 1024 * 1024 + 1])
      .send()
      .await
      .unwrap()
      .status(),
    413
  );
  let config = support::config(&app, Protocol::ChatCompletions);
  assert_eq!(
    model_provider::classify_json(&config, "synthetic", CancellationToken::new())
      .await
      .unwrap(),
    "intact"
  );
}

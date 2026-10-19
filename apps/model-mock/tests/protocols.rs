mod support;
use model_mock::{Protocol, Reply};
use rpi_ai::{
  AssistantMessageEvent as Event, Content, Context, DoneReason, SimpleStreamOptions,
  ToolResultMessage, UserMessage,
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
    let (model, provider) = model_provider::configured(&config).unwrap();
    let mut context = Context::new(vec![UserMessage::new("call fixture_echo", 0).into()]);
    let options = SimpleStreamOptions {
      api_key: Some(config.api_key.clone()),
      max_retries: Some(0),
      ..Default::default()
    };
    let mut stream = provider.stream_simple(&model, &context, &options).await;
    let mut tool = None;
    let mut terminal = None;
    while let Some(event) = stream.next().await {
      match event {
        Event::ToolCallEnd { tool_call, .. } => tool = Some(tool_call),
        Event::Done { reason, message } => {
          assert_eq!(reason, DoneReason::ToolUse);
          terminal = Some(message);
        }
        Event::Error { error, .. } => panic!("adapter error: {error:?}"),
        _ => {}
      }
    }
    let tool = tool.unwrap();
    assert_eq!(tool.name, "fixture_echo");
    assert_eq!(tool.arguments, json!({"text":"synthetic"}));
    context.messages.push(terminal.unwrap().into());
    context.messages.push(
      ToolResultMessage {
        role: Default::default(),
        tool_call_id: tool.id,
        tool_name: tool.name,
        content: vec![Content::text("synthetic")],
        details: None,
        usage: None,
        added_tool_names: Vec::new(),
        is_error: false,
        timestamp: 0,
      }
      .into(),
    );
    let mut stream = provider.stream_simple(&model, &context, &options).await;
    let mut final_text = String::new();
    let mut stopped = false;
    while let Some(event) = stream.next().await {
      match event {
        Event::TextDelta { delta, .. } => final_text.push_str(&delta),
        Event::Done { reason, .. } => {
          assert_eq!(reason, DoneReason::Stop);
          stopped = true;
        }
        Event::Error { error, .. } => panic!("adapter error: {error:?}"),
        _ => {}
      }
    }
    assert!(stopped);
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

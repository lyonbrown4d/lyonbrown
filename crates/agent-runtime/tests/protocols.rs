#[allow(dead_code)]
mod support;
use agent_runtime::{
  RunOutcome, Runtime, RuntimeEvent, RuntimeImage, RuntimeTool, ToolCall, ToolDefinition,
  ToolOutput,
};
use agent_test_support::{
  MockModelServer, ScriptedReply, StandardProtocol, text_reply, tool_reply, truncated_reply,
};
use serde_json::json;
use std::sync::{
  Arc, Mutex,
  atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;
const PNG: &str =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
struct Inspect(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl RuntimeTool for Inspect {
  fn definition(&self) -> ToolDefinition {
    ToolDefinition {
      name: "inspect".into(),
      description: "Read fixture file".into(),
      parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
    }
  }
  async fn execute(
    &self,
    call: ToolCall,
    _: CancellationToken,
  ) -> agent_runtime::Result<ToolOutput> {
    assert_eq!(call.arguments, json!({"path":"fixture.txt"}));
    self.0.fetch_add(1, Ordering::SeqCst);
    Ok(ToolOutput {
      text: "fixture file contents".into(),
      is_error: false,
    })
  }
}
fn config(protocol: StandardProtocol, server: &MockModelServer) -> agent_runtime::RuntimeConfig {
  let mut config = support::config(server.base_url());
  config.provider = protocol.provider().into();
  config
}
async fn contract(protocol: StandardProtocol) {
  let server = MockModelServer::start(vec![
    tool_reply(protocol, "inspect", json!({"path":"fixture.txt"})),
    text_reply(protocol, "mock result ✓"),
  ])
  .await
  .unwrap();
  let calls = Arc::new(AtomicUsize::new(0));
  let events = Arc::new(Mutex::new(Vec::new()));
  let capture = events.clone();
  let store = support::shared();
  let runtime = Runtime::open(
    config(protocol, &server),
    store.clone(),
    vec![Arc::new(Inspect(calls.clone()))],
    Arc::new(move |event| capture.lock().unwrap().push(event)),
  )
  .await
  .unwrap();
  let result = runtime
    .execute_input(
      "inspect fixture",
      vec![RuntimeImage {
        mime_type: "image/png".into(),
        data_base64: PNG.into(),
      }],
    )
    .await
    .unwrap();
  assert_eq!(result.outcome, RunOutcome::Completed, "{protocol:?}");
  assert_eq!(result.text, "mock result ✓", "{protocol:?}");
  assert_eq!(
    (
      result.usage.input_tokens,
      result.usage.output_tokens,
      result.usage.total_tokens
    ),
    (11, 3, 14),
    "{protocol:?}"
  );
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  let text = events
    .lock()
    .unwrap()
    .iter()
    .filter_map(|event| {
      if let RuntimeEvent::TextDelta { text } = event {
        Some(text.clone())
      } else {
        None
      }
    })
    .collect::<String>();
  assert_eq!(text, "mock result ✓");
  let requests = server.requests().await;
  assert_eq!(requests.len(), 2);
  for request in &requests {
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, protocol.path());
    assert_eq!(request.body["model"], "fixture-model");
    assert_eq!(request.body["stream"], true);
    let authenticated = if protocol == StandardProtocol::Anthropic {
      request
        .headers
        .get("x-api-key")
        .is_some_and(|value| value == "fixture-secret-never-persist")
    } else {
      request
        .headers
        .get("authorization")
        .is_some_and(|value| value == "Bearer fixture-secret-never-persist")
    };
    assert!(
      authenticated,
      "{protocol:?} omitted expected authentication"
    );
  }
  let first = &requests[0].body;
  let second = &requests[1].body;
  match protocol {
    StandardProtocol::ChatCompletions => {
      assert_eq!(
        first["tools"][0]["function"]["parameters"]["required"],
        json!(["path"])
      );
      assert!(first["messages"].as_array().unwrap().iter().any(|message| {
        message["content"].as_array().is_some_and(|parts| {
          parts.iter().any(|part| {
            part["type"] == "image_url"
              && part["image_url"]["url"]
                .as_str()
                .is_some_and(|value| value.starts_with("data:image/png;base64,"))
          })
        })
      }));
      assert!(second["messages"].as_array().unwrap().iter().any(|message|message["role"] == "tool" && message["content"] == "fixture file contents"));
    }
    StandardProtocol::Responses => {
      assert_eq!(first["tools"][0]["parameters"]["required"], json!(["path"]));
      assert!(first["input"].as_array().unwrap().iter().any(|message| {
        message["content"]
          .as_array()
          .is_some_and(|parts| parts.iter().any(|part| part["type"] == "input_image"))
      }));
      assert!(
        second["input"]
          .as_array()
          .unwrap()
          .iter()
          .any(|item| item["type"] == "function_call_output"
            && item["output"] == "fixture file contents")
      );
    }
    StandardProtocol::Anthropic => {
      assert!(requests[0].headers.contains_key("anthropic-version"));
      assert_eq!(
        first["tools"][0]["input_schema"]["required"],
        json!(["path"])
      );
      assert!(first["messages"].as_array().unwrap().iter().any(|message| {
        message["content"].as_array().is_some_and(|parts| {
          parts
            .iter()
            .any(|part| part["type"] == "image" && part["source"]["type"] == "base64")
        })
      }));
      assert!(
        second["messages"]
          .as_array()
          .unwrap()
          .iter()
          .any(|message| message["content"]
            .as_array()
            .is_some_and(
              |parts| parts.iter().any(|part| part["type"] == "tool_result"
                && part["content"]
                  .to_string()
                  .contains("fixture file contents"))
            ))
      );
    }
  }
  assert!(
    !String::from_utf8(store.snapshot.lock().unwrap().clone().unwrap())
      .unwrap()
      .contains("fixture-secret-never-persist")
  );
}
async fn failures(protocol: StandardProtocol) {
  let malformed = if protocol == StandardProtocol::Anthropic {
    b"event: message_start\ndata: not-json\n\n".to_vec()
  } else {
    b"data: not-json\n\n".to_vec()
  };
  for reply in [
    ScriptedReply::status(429, json!({"error":{"message":"synthetic rate limit"}}))
      .header("retry-after", "0"),
    ScriptedReply::status(503, json!({"error":{"message":"synthetic outage"}})),
    ScriptedReply::raw_sse(malformed),
    truncated_reply(protocol),
  ] {
    // A failed continuation must not repeat the already executed side effect.
    let server = MockModelServer::start(vec![
      tool_reply(protocol, "inspect", json!({"path":"fixture.txt"})),
      reply,
    ])
    .await
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let runtime = Runtime::open(
      config(protocol, &server),
      support::shared(),
      vec![Arc::new(Inspect(calls.clone()))],
      Arc::new(|_| {}),
    )
    .await
    .unwrap();
    let result = runtime.execute("inspect fixture").await.unwrap();
    assert!(
      matches!(result.outcome, RunOutcome::Failed { .. }),
      "{protocol:?} accepted a failed or incomplete stream"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
      server.requests().await.len(),
      2,
      "{protocol:?} retried blindly"
    );
  }
}
async fn cancellation(protocol: StandardProtocol) {
  let server = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let runtime = Arc::new(
    Runtime::open(
      config(protocol, &server),
      support::shared(),
      Vec::new(),
      Arc::new(|_| {}),
    )
    .await
    .unwrap(),
  );
  let task = {
    let runtime = runtime.clone();
    tokio::spawn(async move { runtime.execute("wait").await })
  };
  tokio::time::timeout(
    std::time::Duration::from_secs(5),
    server.wait_for_requests(1),
  )
  .await
  .unwrap();
  runtime.cancel().await.unwrap();
  let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
    .await
    .unwrap()
    .unwrap()
    .unwrap();
  assert_eq!(result.outcome, RunOutcome::Cancelled, "{protocol:?}");
  assert_eq!(server.requests().await.len(), 1);
}
macro_rules! protocol_tests {
  ($contract:ident,$failures:ident,$cancel:ident,$protocol:expr) => {
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn $contract() {
      contract($protocol).await;
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn $failures() {
      failures($protocol).await;
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn $cancel() {
      cancellation($protocol).await;
    }
  };
}
protocol_tests!(
  chat_contract,
  chat_failures,
  chat_cancellation,
  StandardProtocol::ChatCompletions
);
protocol_tests!(
  responses_contract,
  responses_failures,
  responses_cancellation,
  StandardProtocol::Responses
);
protocol_tests!(
  anthropic_contract,
  anthropic_failures,
  anthropic_cancellation,
  StandardProtocol::Anthropic
);

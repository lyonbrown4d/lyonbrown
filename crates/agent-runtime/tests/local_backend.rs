#[allow(dead_code)]
mod support;
use agent_inference::{
  ChatMessage, FinishReason, GenerationRequest, GenerationSummary, InferenceBackend,
  InferenceChunk, InputContent, MessageRole, ModelInfo, ModelRegistry, ToolCall,
};
use agent_runtime::{RunOutcome, Runtime, RuntimeImage};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct StructuredModel {
  requests: Mutex<Vec<GenerationRequest>>,
}
#[async_trait::async_trait]
impl InferenceBackend for StructuredModel {
  fn info(&self) -> ModelInfo {
    ModelInfo {
      id: "fixture-model".into(),
      backend: "deterministic-test".into(),
      context_window: 2048,
      supports_tools: true,
      supports_images: true,
    }
  }
  async fn generate(
    &self,
    request: GenerationRequest,
    _: CancellationToken,
    output: mpsc::Sender<InferenceChunk>,
  ) -> agent_inference::Result<GenerationSummary> {
    self.requests.lock().unwrap().push(request);
    let first = self.requests.lock().unwrap().len() == 1;
    let chunk = if first {
      InferenceChunk::ToolCall {
        call: ToolCall {
          id: "call-local".into(),
          name: "inspect".into(),
          arguments: serde_json::json!({"path":"file.txt"}),
        },
      }
    } else {
      InferenceChunk::Text {
        text: "local result".into(),
      }
    };
    output.send(chunk).await.unwrap();
    Ok(GenerationSummary {
      reason: if first {
        FinishReason::ToolCalls
      } else {
        FinishReason::Stop
      },
      input_tokens: 10,
      output_tokens: 2,
    })
  }
}
struct Inspect;
#[async_trait::async_trait]
impl agent_runtime::RuntimeTool for Inspect {
  fn definition(&self) -> agent_runtime::ToolDefinition {
    agent_runtime::ToolDefinition {
      name: "inspect".into(),
      description: "Inspect".into(),
      parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
    }
  }
  async fn execute(
    &self,
    _: agent_runtime::ToolCall,
    _: CancellationToken,
  ) -> agent_runtime::Result<agent_runtime::ToolOutput> {
    Ok(agent_runtime::ToolOutput {
      text: "local tool result".into(),
      is_error: false,
    })
  }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_inference_bridge_preserves_images_tools_and_tool_results() {
  let model = Arc::new(StructuredModel {
    requests: Mutex::new(Vec::new()),
  });
  let registry = Arc::new(ModelRegistry::new(1).unwrap());
  registry.register(model.clone(), 1).await.unwrap();
  let backend = registry.backend("fixture-model").await.unwrap();
  let mut config = support::config("http://unused.invalid");
  config.max_output_tokens = 256;
  let runtime = Runtime::open_with_backend(
    config,
    support::shared(),
    vec![Arc::new(Inspect)],
    Arc::new(|_| {}),
    backend,
  )
  .await
  .unwrap();
  assert!(
    runtime
      .execute_input(
        "inspect",
        vec![RuntimeImage {
          mime_type: "image/png".into(),
          data_base64: "invalid".into()
        }]
      )
      .await
      .is_err()
  );
  assert!(model.requests.lock().unwrap().is_empty());
  let result = runtime
    .execute_input(
      "inspect",
      vec![RuntimeImage {
        mime_type: "image/png".into(),
        data_base64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC".into(),
      }],
    )
    .await
    .unwrap();
  assert_eq!(result.outcome, RunOutcome::Completed);
  assert_eq!(result.text, "local result");
  let requests = model.requests.lock().unwrap();
  assert_eq!(requests.len(), 2);
  assert!(requests[0].messages.iter().any(|message|message.content.iter().any(|content|matches!(content,InputContent::Image{data_base64,..} if data_base64 == "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC"))));
  assert_eq!(requests[0].tools[0].name, "inspect");
  assert!(requests[1].messages.iter().any(
    |ChatMessage {
       role, tool_calls, ..
     }| *role == MessageRole::Assistant && tool_calls.iter().any(|call| call.name == "inspect")
  ));
  assert!(
    requests[1]
      .messages
      .iter()
      .any(|message| message.role == MessageRole::Tool
        && message.tool_call_id.as_deref() == Some("call-local"))
  );
}

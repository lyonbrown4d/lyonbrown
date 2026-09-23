use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_core::{
  ContextItem, ContextItemKind, DataClassification, Locality, ModelCapabilities, ModelTier,
  SecretRef, StopReason, TokenUsage, ToolCallId, ToolInvocation,
};
use agent_kernel::Kernel;
use agent_model::{
  ModelError, ModelEvent, ModelProvider, ModelRegistry, ModelRequest, ModelStream,
};
use agent_secret::{SecretBroker, SecretError};
use agent_tool::{EchoTool, ToolRegistry};
use async_trait::async_trait;
use futures::stream;

#[path = "fallback/stream_start.rs"]
mod stream_start;

struct Provider {
  id: &'static str,
  locality: Locality,
  tier: ModelTier,
  failure: Option<u16>,
}

#[async_trait]
impl ModelProvider for Provider {
  fn provider_id(&self) -> &'static str {
    self.id
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Ok(vec![ModelCapabilities {
      provider: self.id.into(),
      model: format!("{}-model", self.id),
      tier: self.tier,
      locality: self.locality,
      context_window: 32_768,
      max_output_tokens: 1_024,
      supports_tools: true,
      supports_streaming: true,
      supports_json: true,
      supports_thinking: false,
      cost_per_mtok_in: 0.0,
      cost_per_mtok_out: 0.0,
    }])
  }

  async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
    if let Some(status) = self.failure {
      return if status == 0 {
        Err(ModelError::Timeout)
      } else {
        Err(ModelError::Http(
          status,
          if status == 401 {
            "secret-value"
          } else {
            "unavailable"
          }
          .into(),
        ))
      };
    }
    Ok(ModelStream {
      model: request.model,
      events: Box::pin(stream::iter(vec![
        Ok(ModelEvent::Delta("remote answer".into())),
        Ok(ModelEvent::Usage(TokenUsage::default())),
        Ok(ModelEvent::Done {
          stop_reason: StopReason::EndTurn,
        }),
      ])),
    })
  }
}

struct RedactingBroker;

#[async_trait]
impl SecretBroker for RedactingBroker {
  async fn resolve(&self, reference: &SecretRef) -> Result<String, SecretError> {
    Err(SecretError::NotFound(reference.clone()))
  }

  fn redact(&self, text: &str) -> String {
    text.replace("secret-value", "[REDACTED]")
  }
}

#[tokio::test]
async fn provider_error_body_is_redacted_from_returned_error_and_event_log() {
  let registry = ModelRegistry::new();
  registry.register(Arc::new(Provider {
    id: "local",
    locality: Locality::Local,
    tier: ModelTier::Worker,
    failure: Some(401),
  }));
  let mut kernel = Kernel::builder()
    .providers(registry)
    .broker(RedactingBroker)
    .build();
  let error = kernel.run_turn("help me").await.unwrap_err();
  assert!(!error.to_string().contains("secret-value"));
  assert!(
    !serde_json::to_string(kernel.event_log())
      .unwrap()
      .contains("secret-value")
  );
}

fn kernel_with_local_failure(status: u16) -> Kernel {
  let registry = ModelRegistry::new();
  registry.register(Arc::new(Provider {
    id: "local",
    locality: Locality::Local,
    tier: ModelTier::Worker,
    failure: Some(status),
  }));
  registry.register(Arc::new(Provider {
    id: "remote",
    locality: Locality::Remote,
    tier: ModelTier::Frontier,
    failure: None,
  }));
  Kernel::builder().providers(registry).build()
}

#[tokio::test]
async fn local_timeout_retries_remote_frontier_once() {
  let mut kernel = kernel_with_local_failure(0);
  let events = kernel.run_turn("help me").await.unwrap();
  assert_eq!(
    events.iter().find_map(|event| event.turn_answer()),
    Some("remote answer")
  );
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind() == "ModelFallback")
      .count(),
    1
  );
  assert!(events.iter().any(|event| matches!(event,
    agent_core::AgentEvent::ModelSelected { provider, reason, .. }
      if provider == "remote" && reason.contains("fallback"))));
  let requests = kernel.last_requests();
  assert_eq!(requests.len(), 2);
  assert!(
    requests[0]
      .system
      .as_deref()
      .unwrap()
      .contains("local/local-model")
  );
  assert!(
    requests[1]
      .system
      .as_deref()
      .unwrap()
      .contains("remote/remote-model")
  );
}

#[tokio::test]
async fn local_5xx_retries_remote_but_auth_failure_does_not() {
  let mut unavailable = kernel_with_local_failure(503);
  assert!(unavailable.run_turn("help me").await.is_ok());

  let mut unauthorized = kernel_with_local_failure(401);
  assert!(unauthorized.run_turn("help me").await.is_err());
  assert!(
    !unauthorized
      .event_log()
      .iter()
      .any(|event| event.kind() == "ModelFallback")
  );
}

#[tokio::test]
async fn confidential_context_is_not_sent_by_automatic_fallback() {
  let mut context = ContextItem::new(ContextItemKind::Document, "workspace secret", 0);
  context.classification = DataClassification::Confidential;
  let registry = ModelRegistry::new();
  registry.register(Arc::new(Provider {
    id: "local",
    locality: Locality::Local,
    tier: ModelTier::Worker,
    failure: Some(503),
  }));
  registry.register(Arc::new(Provider {
    id: "remote",
    locality: Locality::Remote,
    tier: ModelTier::Frontier,
    failure: None,
  }));
  let mut kernel = Kernel::builder()
    .providers(registry)
    .context_items([context])
    .build();

  assert!(kernel.run_turn("summarize").await.is_err());
  assert!(
    !kernel
      .event_log()
      .iter()
      .any(|event| event.kind() == "ModelFallback")
  );
  assert_eq!(kernel.last_requests().len(), 1);
}

struct ToolThenTimeout(AtomicUsize);

#[async_trait]
impl ModelProvider for ToolThenTimeout {
  fn provider_id(&self) -> &'static str {
    "local"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Provider {
      id: "local",
      locality: Locality::Local,
      tier: ModelTier::Worker,
      failure: None,
    }
    .list_models()
    .await
  }

  async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
    if self.0.fetch_add(1, Ordering::SeqCst) > 0 {
      return Err(ModelError::Timeout);
    }
    Ok(ModelStream {
      model: request.model,
      events: Box::pin(stream::iter(vec![
        Ok(ModelEvent::ToolCall(ToolInvocation {
          id: ToolCallId::new(),
          name: "echo".into(),
          arguments: serde_json::json!({"text": "private file contents"}),
        })),
        Ok(ModelEvent::Done {
          stop_reason: StopReason::ToolUse,
        }),
      ])),
    })
  }
}

#[tokio::test]
async fn tool_observation_blocks_automatic_remote_fallback() {
  let registry = ModelRegistry::new();
  registry.register(Arc::new(ToolThenTimeout(AtomicUsize::new(0))));
  registry.register(Arc::new(Provider {
    id: "remote",
    locality: Locality::Remote,
    tier: ModelTier::Frontier,
    failure: None,
  }));
  let tools = ToolRegistry::new();
  tools.register(Arc::new(EchoTool::new()));
  let mut kernel = Kernel::builder().providers(registry).tools(tools).build();

  assert!(kernel.run_turn("run a shell command").await.is_err());

  assert!(kernel.event_log().iter().any(|event| matches!(event,
    agent_core::AgentEvent::ToolCompleted { result, .. }
      if result.output == "private file contents")));
  assert!(
    !kernel
      .event_log()
      .iter()
      .any(|event| event.kind() == "ModelFallback")
  );
}

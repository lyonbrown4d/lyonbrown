//! Private conversion between application inference contracts and the harness.
use crate::{Result, RuntimeConfig, RuntimeError, local_stream::StreamState};
use agent_inference::{
  ChatMessage, GenerationRequest, InferenceBackend, InputContent, MessageRole,
};
use async_trait::async_trait;
use rpi_ai::event_stream::create_assistant_message_event_stream_with_cancellation;
use rpi_ai::{
  Api, AssistantMessageEventStream, Content, Context, Model, Provider, SimpleStreamOptions,
  UserContent,
};
use std::sync::Arc;

pub(crate) fn configured(
  config: &RuntimeConfig,
  backend: Arc<dyn InferenceBackend>,
) -> Result<(Model, Arc<dyn Provider>)> {
  let info = backend.info();
  if config.session_id.is_empty()
    || config.owner_epoch == 0
    || config.model != info.id
    || config.max_output_tokens == 0
    || config.max_output_tokens >= info.context_window
  {
    return Err(RuntimeError::Configuration(
      "invalid local model/session identity or token limit".into(),
    ));
  }
  let mut model = Model::new(
    &info.id,
    &info.id,
    Api::OpenaiCompletions,
    "lyonbrown-local",
    "local://inference",
  );
  model.context_window = u64::from(info.context_window);
  model.max_tokens = u64::from(config.max_output_tokens);
  if info.supports_images {
    model.input.push(rpi_ai::InputModality::Image);
  }
  let provider = LocalProvider {
    backend,
    models: vec![model.clone()],
  };
  Ok((model, Arc::new(provider)))
}
struct LocalProvider {
  backend: Arc<dyn InferenceBackend>,
  models: Vec<Model>,
}
#[async_trait]
impl Provider for LocalProvider {
  fn id(&self) -> &str {
    "lyonbrown-local"
  }
  fn models(&self) -> &[Model] {
    &self.models
  }
  async fn stream_simple(
    &self,
    model: &Model,
    context: &Context,
    options: &SimpleStreamOptions,
  ) -> AssistantMessageEventStream {
    let (producer, stream) =
      create_assistant_message_event_stream_with_cancellation(&options.signal);
    let request = convert_request(context, model, options);
    let backend = self.backend.clone();
    let model = model.clone();
    let cancellation = options.signal.child_token();
    tokio::spawn(async move {
      let mut state = StreamState::new(model, producer).await;
      let request = match request {
        Ok(request) => request,
        Err(error) => {
          state.fail(error.to_string(), false).await;
          return;
        }
      };
      let (sender, mut receiver) = tokio::sync::mpsc::channel(64);
      let worker_cancel = cancellation.clone();
      let worker =
        tokio::spawn(async move { backend.generate(request, worker_cancel, sender).await });
      while let Some(chunk) = receiver.recv().await {
        if !state.chunk(chunk).await {
          cancellation.cancel();
          break;
        }
      }
      match worker.await {
        Ok(Ok(summary)) => state.finish(summary).await,
        Ok(Err(error)) => {
          state
            .fail(
              error.to_string(),
              matches!(error, agent_inference::InferenceError::Cancelled),
            )
            .await
        }
        Err(error) => {
          state
            .fail(format!("local inference worker stopped: {error}"), false)
            .await
        }
      }
    });
    stream
  }
}
fn convert_request(
  context: &Context,
  model: &Model,
  options: &SimpleStreamOptions,
) -> Result<GenerationRequest> {
  let mut messages = Vec::new();
  if let Some(system) = &context.system_prompt {
    messages.push(ChatMessage {
      role: MessageRole::System,
      content: vec![InputContent::Text {
        text: system.clone(),
      }],
      tool_calls: Vec::new(),
      tool_call_id: None,
    });
  }
  for message in &context.messages {
    let (role, content, tool_calls, tool_call_id) = match message {
      rpi_ai::Message::User(message) => (
        MessageRole::User,
        match &message.content {
          UserContent::Text(text) => vec![InputContent::Text { text: text.clone() }],
          UserContent::Blocks(blocks) => input_content(blocks),
        },
        Vec::new(),
        None,
      ),
      rpi_ai::Message::Assistant(message) => (
        MessageRole::Assistant,
        input_content(&message.content),
        message
          .content
          .iter()
          .filter_map(|content| match content {
            Content::ToolCall(call) => Some(agent_inference::ToolCall {
              id: call.id.clone(),
              name: call.name.clone(),
              arguments: call.arguments.clone(),
            }),
            _ => None,
          })
          .collect(),
        None,
      ),
      rpi_ai::Message::ToolResult(message) => (
        MessageRole::Tool,
        input_content(&message.content),
        Vec::new(),
        Some(message.tool_call_id.clone()),
      ),
    };
    messages.push(ChatMessage {
      role,
      content,
      tool_calls,
      tool_call_id,
    });
  }
  let max_tokens = u32::try_from(options.max_tokens.unwrap_or(model.max_tokens))
    .map_err(|error| RuntimeError::Configuration(error.to_string()))?;
  Ok(GenerationRequest {
    messages,
    tools: context
      .tools
      .iter()
      .map(|tool| agent_inference::ToolDefinition {
        name: tool.name.clone(),
        description: tool.description.clone(),
        parameters: tool.parameters.0.clone(),
      })
      .collect(),
    max_tokens,
    temperature: options.temperature,
    seed: 0,
  })
}
fn input_content(content: &[Content]) -> Vec<InputContent> {
  content
    .iter()
    .filter_map(|content| match content {
      Content::Text(text) => Some(InputContent::Text {
        text: text.text.clone(),
      }),
      Content::Image(image) => Some(InputContent::Image {
        mime: image.mime_type.clone(),
        data_base64: image.data.clone(),
      }),
      _ => None,
    })
    .collect()
}

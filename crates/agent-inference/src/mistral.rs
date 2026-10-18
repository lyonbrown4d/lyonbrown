use crate::{
  FinishReason, GenerationRequest, GenerationSummary, InferenceBackend, InferenceChunk,
  InferenceError, InputContent, MessageRole, ModelInfo, Result, ToolCall,
};
use async_trait::async_trait;
use mistralrs::{Model, RequestBuilder, Response, TextMessageRole};
use std::{collections::BTreeMap, path::Path};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct MistralBackend {
  model: Model,
  info: ModelInfo,
}
impl MistralBackend {
  /// Load a complete local model directory without fetching model files.
  pub async fn load(
    id: String,
    path: impl AsRef<Path>,
    context_window: u32,
    supports_tools: bool,
    supports_images: bool,
    force_cpu: bool,
  ) -> Result<Self> {
    let (model, info) = crate::mistral_load::directory(
      id,
      path.as_ref(),
      context_window,
      supports_tools,
      supports_images,
      force_cpu,
    )
    .await?;
    Ok(Self { model, info })
  }
  /// Load a local GGUF using the same inference-only SDK path.
  pub async fn load_gguf(
    id: String,
    path: impl AsRef<Path>,
    context_window: u32,
    supports_tools: bool,
    force_cpu: bool,
  ) -> Result<Self> {
    let (model, info) =
      crate::mistral_load::gguf(id, path.as_ref(), context_window, supports_tools, force_cpu)
        .await?;
    Ok(Self { model, info })
  }
}
#[async_trait]
impl InferenceBackend for MistralBackend {
  fn info(&self) -> ModelInfo {
    self.info.clone()
  }
  async fn generate(
    &self,
    request: GenerationRequest,
    cancellation: CancellationToken,
    output: mpsc::Sender<InferenceChunk>,
  ) -> Result<GenerationSummary> {
    crate::contracts::validate(&self.info, &request)?;
    let builder = build_request(request)?;
    let mut stream = tokio::select! {biased;_=cancellation.cancelled()=>return Err(InferenceError::Cancelled),stream=self.model.stream_chat_request(builder)=>stream.map_err(backend_error)?};
    let mut summary = GenerationSummary {
      reason: FinishReason::Stop,
      input_tokens: 0,
      output_tokens: 0,
    };
    let mut calls: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
    let mut terminal = false;
    loop {
      let response = tokio::select! {biased;_=cancellation.cancelled()=>return Err(InferenceError::Cancelled),response=stream.next()=>response};
      match response {
        Some(Response::Chunk(chunk)) => {
          if let Some(usage) = chunk.usage {
            summary.input_tokens = usage.prompt_tokens as u64;
            summary.output_tokens = usage.completion_tokens as u64;
          }
          for choice in chunk.choices {
            if choice.index != 0 {
              continue;
            }
            if let Some(text) = choice.delta.content {
              crate::contracts::emit(&output, &cancellation, InferenceChunk::Text { text }).await?;
            }
            if let Some(text) = choice.delta.reasoning_content {
              crate::contracts::emit(&output, &cancellation, InferenceChunk::Thinking { text })
                .await?;
            }
            for call in choice.delta.tool_calls.unwrap_or_default() {
              let entry = calls.entry(call.index).or_default();
              if !call.id.is_empty() {
                entry.0 = call.id;
              }
              if entry.1 != call.function.name {
                entry.1.push_str(&call.function.name);
              }
              entry.2.push_str(&call.function.arguments);
            }
            if let Some(reason) = choice.finish_reason {
              terminal = true;
              summary.reason = match reason.as_str() {
                "tool_calls" => FinishReason::ToolCalls,
                "length" => FinishReason::Length,
                _ => FinishReason::Stop,
              };
            }
          }
        }
        Some(Response::InternalError(error) | Response::ValidationError(error)) => {
          return Err(backend_error(error));
        }
        Some(Response::ModelError(error, _)) => return Err(backend_error(error)),
        Some(Response::Done(response)) => {
          terminal = true;
          summary.input_tokens = response.usage.prompt_tokens as u64;
          summary.output_tokens = response.usage.completion_tokens as u64;
          break;
        }
        None => break,
        _ => {
          return Err(InferenceError::Backend(
            "unexpected response from chat inference".into(),
          ));
        }
      }
    }
    if !terminal {
      return Err(InferenceError::Backend(
        "model stream ended without a terminal response".into(),
      ));
    }
    if !calls.is_empty() {
      summary.reason = FinishReason::ToolCalls;
    }
    for (_, (id, name, arguments)) in calls {
      let arguments = serde_json::from_str(&arguments).map_err(backend_error)?;
      crate::contracts::emit(
        &output,
        &cancellation,
        InferenceChunk::ToolCall {
          call: ToolCall {
            id,
            name,
            arguments,
          },
        },
      )
      .await?;
    }
    Ok(summary)
  }
}
fn build_request(request: GenerationRequest) -> Result<RequestBuilder> {
  if request.seed != 0 && request.temperature.is_some_and(|value| value > 0.0) {
    return Err(InferenceError::InvalidRequest(
      "mistral.rs SDK does not expose a per-request stochastic seed".into(),
    ));
  }
  let sampling = if request.temperature.is_some_and(|value| value > 0.0) {
    mistralrs::SamplingParams::neutral()
  } else {
    mistralrs::SamplingParams::deterministic()
  };
  let mut builder = RequestBuilder::new()
    .set_sampling(sampling)
    .set_sampler_max_len(request.max_tokens as usize);
  if let Some(temperature) = request.temperature.filter(|value| *value > 0.0) {
    builder = builder.set_sampler_temperature(temperature);
  }
  for message in request.messages {
    let role = match message.role {
      MessageRole::System => TextMessageRole::System,
      MessageRole::User => TextMessageRole::User,
      MessageRole::Assistant => TextMessageRole::Assistant,
      MessageRole::Tool => TextMessageRole::Tool,
    };
    let mut text = String::new();
    let mut images = Vec::new();
    for part in message.content {
      match part {
        InputContent::Text { text: part } => text.push_str(&part),
        InputContent::Image { mime, data_base64 } => {
          images.push(decode_image(&mime, &data_base64)?)
        }
      };
    }
    if !images.is_empty() {
      builder = builder.add_image_message(role, text, images);
    } else if message.role == MessageRole::Tool {
      builder = builder.add_tool_message(
        text,
        message
          .tool_call_id
          .ok_or_else(|| InferenceError::InvalidRequest("tool result id is required".into()))?,
      );
    } else if !message.tool_calls.is_empty() {
      let calls = message
        .tool_calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| mistralrs::ToolCallResponse {
          index,
          id: call.id,
          tp: mistralrs::ToolCallType::Function,
          function: mistralrs::CalledFunction {
            name: call.name,
            arguments: call.arguments.to_string(),
          },
        })
        .collect();
      builder = builder.add_message_with_tool_call(role, text, calls);
    } else {
      builder = builder.add_message(role, text);
    }
  }
  if !request.tools.is_empty() {
    let tools=request.tools.into_iter().map(|tool|serde_json::from_value(serde_json::json!({"type":"function","function":{"name":tool.name,"description":tool.description,"parameters":tool.parameters}})).map_err(backend_error)).collect::<Result<Vec<mistralrs::Tool>>>()?;
    builder = builder.set_tools(tools);
  }
  Ok(builder)
}
fn decode_image(mime: &str, data: &str) -> Result<image::DynamicImage> {
  use base64::Engine;
  if !matches!(mime, "image/png" | "image/jpeg") || data.len() > 24 * 1024 * 1024 {
    return Err(InferenceError::InvalidRequest(
      "image must be PNG/JPEG below 16MiB decoded".into(),
    ));
  }
  let bytes = base64::engine::general_purpose::STANDARD
    .decode(data)
    .map_err(backend_error)?;
  let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
    .with_guessed_format()
    .map_err(backend_error)?;
  let mut limits = image::Limits::default();
  limits.max_image_width = Some(8192);
  limits.max_image_height = Some(8192);
  limits.max_alloc = Some(64 * 1024 * 1024);
  reader.limits(limits);
  reader.decode().map_err(backend_error)
}
fn backend_error(error: impl std::fmt::Display) -> InferenceError {
  InferenceError::Backend(error.to_string())
}

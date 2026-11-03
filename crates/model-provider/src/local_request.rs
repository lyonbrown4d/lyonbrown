//! Maps canonical Rig messages to the inference-only application port.
use agent_inference::{ChatMessage, GenerationRequest, InputContent, MessageRole, ModelInfo};
use rig_core::completion::CompletionRequest;
use rig_core::message::{
  AssistantContent, DocumentSourceKind, Image, Message, MimeType, ToolChoice, ToolResultContent,
  UserContent,
};

pub(crate) struct InvalidRequest;

pub(crate) fn convert(
  request: CompletionRequest,
  info: &ModelInfo,
  default_max: u32,
) -> Result<GenerationRequest, InvalidRequest> {
  if request.model.as_deref().is_some_and(|id| id != info.id)
    || request
      .temperature
      .is_some_and(|value| !value.is_finite() || value < 0.0)
    || request.additional_params.is_some()
    || request.output_schema.is_some()
    || !request.provider_options.is_empty()
    || !request.cache_points.is_empty()
    || !matches!(request.tool_choice, None | Some(ToolChoice::Auto))
    || !info.supports_tools && !request.tools.is_empty()
  {
    return Err(invalid());
  }
  let max_tokens =
    u32::try_from(request.max_tokens.unwrap_or(u64::from(default_max))).map_err(|_| invalid())?;
  if max_tokens == 0 || max_tokens >= info.context_window || max_tokens > default_max {
    return Err(invalid());
  }
  let mut messages = Vec::new();
  for message in &request.chat_history {
    match &**message {
      Message::System { content } => messages.push(text_message(MessageRole::System, content)),
      Message::User { content } => {
        // Results are separate tool turns; preserve interleaved user content order.
        let mut user = empty_message(MessageRole::User);
        for part in content {
          match part {
            UserContent::Text(text) => user.content.push(text_content(&text.text)),
            UserContent::Image(value) => user.content.push(image(value, info)?),
            UserContent::ToolResult(result) => {
              if !info.supports_tools {
                return Err(invalid());
              }
              if !user.content.is_empty() {
                messages.push(user);
                user = empty_message(MessageRole::User);
              }
              let mut tool = empty_message(MessageRole::Tool);
              tool.tool_call_id = Some(result.call.wire().into_owned());
              for content in &result.content {
                tool.content.push(match content {
                  ToolResultContent::Text(text) => text_content(&text.text),
                  ToolResultContent::Json { value } => text_content(&value.to_string()),
                  ToolResultContent::Image(value) => image(value, info)?,
                });
              }
              messages.push(tool);
            }
            _ => return Err(invalid()),
          }
        }
        if !user.content.is_empty() {
          messages.push(user);
        }
      }
      Message::Assistant(value) => {
        let mut assistant = empty_message(MessageRole::Assistant);
        for part in &value.content {
          match part {
            AssistantContent::Text(text) => assistant.content.push(text_content(&text.text)),
            AssistantContent::Reasoning(value) if !value.redacted => {
              assistant.content.push(text_content(&value.text))
            }
            AssistantContent::ToolCall(call)
              if info.supports_tools && call.function.invalid_arguments.is_none() =>
            {
              assistant.tool_calls.push(agent_inference::ToolCall {
                id: call.id.wire().into_owned(),
                name: call.function.name.as_str().to_owned(),
                arguments: serde_json::Value::Object(call.function.arguments.clone()),
              });
            }
            _ => return Err(invalid()),
          }
        }
        messages.push(assistant);
      }
    }
  }
  Ok(GenerationRequest {
    messages,
    tools: request
      .tools
      .iter()
      .map(|tool| agent_inference::ToolDefinition {
        name: tool.name.as_str().to_owned(),
        description: tool.description.clone(),
        parameters: tool.parameters.clone(),
      })
      .collect(),
    max_tokens,
    temperature: request.temperature,
    seed: request.options.seed.unwrap_or(0),
  })
}

fn text_message(role: MessageRole, text: &str) -> ChatMessage {
  let mut message = empty_message(role);
  message.content.push(text_content(text));
  message
}
fn empty_message(role: MessageRole) -> ChatMessage {
  ChatMessage {
    role,
    content: Vec::new(),
    tool_calls: Vec::new(),
    tool_call_id: None,
  }
}
fn text_content(text: &str) -> InputContent {
  InputContent::Text { text: text.into() }
}

fn image(value: &Image, info: &ModelInfo) -> Result<InputContent, InvalidRequest> {
  if !info.supports_images {
    return Err(invalid());
  }
  let DocumentSourceKind::Base64(data) = &value.data else {
    return Err(invalid());
  };
  let mime = value
    .media_type
    .as_ref()
    .map(MimeType::to_mime_type)
    .ok_or_else(invalid)?;
  Ok(InputContent::Image {
    mime: mime.into(),
    data_base64: data.clone(),
  })
}
fn invalid() -> InvalidRequest {
  InvalidRequest
}

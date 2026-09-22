use std::pin::Pin;

use agent_core::{StopReason, TokenUsage, ToolCallId, ToolInvocation};
use agent_model::{ChatMessage, ModelError, ModelEvent, ModelRequest, Role};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Serialize)]
pub(crate) struct ChatRequest {
    model: String,
    messages: Vec<WireMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    options: WireOptions,
}

#[derive(Serialize, Deserialize, Default)]
struct WireMessage {
    role: String,
    #[serde(default)]
    content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<WireToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Serialize)]
struct WireTool {
    #[serde(rename = "type")]
    kind: &'static str,
    function: WireFunctionDefinition,
}

#[derive(Serialize)]
struct WireFunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Serialize, Deserialize)]
struct WireToolCall {
    function: WireFunctionCall,
}

#[derive(Serialize, Deserialize)]
struct WireFunctionCall {
    name: String,
    arguments: Value,
}

#[derive(Serialize)]
struct WireOptions {
    num_predict: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stop: Vec<String>,
}

#[derive(Deserialize)]
struct ChatChunk {
    error: Option<String>,
    #[serde(default)]
    message: WireMessage,
    #[serde(default)]
    done: bool,
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    eval_count: u32,
}

pub(crate) fn to_wire_request(request: ModelRequest) -> ChatRequest {
    let mut messages =
        Vec::with_capacity(request.messages.len() + usize::from(request.system.is_some()));
    if let Some(system) = request.system {
        messages.push(WireMessage {
            role: "system".into(),
            content: system,
            ..Default::default()
        });
    }
    messages.extend(request.messages.into_iter().map(to_wire_message));

    ChatRequest {
        model: request.model,
        messages,
        stream: true,
        tools: request
            .tools
            .into_iter()
            .map(|tool| WireTool {
                kind: "function",
                function: WireFunctionDefinition {
                    name: tool.name,
                    description: tool.description,
                    parameters: tool.input_schema,
                },
            })
            .collect(),
        format: request.json_mode.then_some("json"),
        think: request.thinking,
        options: WireOptions {
            num_predict: request.max_tokens,
            temperature: request.temperature,
            stop: request.stop,
        },
    }
}

fn to_wire_message(message: ChatMessage) -> WireMessage {
    WireMessage {
        role: match message.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
        .into(),
        content: message.content,
        tool_calls: message
            .tool_calls
            .into_iter()
            .map(|call| WireToolCall {
                function: WireFunctionCall {
                    name: call.name,
                    arguments: call.arguments,
                },
            })
            .collect(),
        tool_name: message.tool_name,
    }
}

pub(crate) fn into_model_stream(
    response: reqwest::Response,
) -> Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send>> {
    let (sender, receiver) = mpsc::channel(16);
    tokio::spawn(async move {
        let mut bytes = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(chunk) => {
                    buffer.extend_from_slice(&chunk);
                    while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                        let line: Vec<_> = buffer.drain(..=newline).collect();
                        if emit_line(&sender, &line[..line.len() - 1]).await.is_err() {
                            return;
                        }
                    }
                }
                Err(error) => {
                    let _ = sender
                        .send(Err(ModelError::Connection(error.to_string())))
                        .await;
                    return;
                }
            }
        }
        if !buffer.is_empty() {
            let _ = emit_line(&sender, &buffer).await;
        }
    });
    Box::pin(ReceiverStream::new(receiver))
}

async fn emit_line(
    sender: &mpsc::Sender<Result<ModelEvent, ModelError>>,
    line: &[u8],
) -> Result<(), ()> {
    let line = trim_ascii_whitespace(line);
    if line.is_empty() {
        return Ok(());
    }
    let chunk: ChatChunk = match serde_json::from_slice(line) {
        Ok(chunk) => chunk,
        Err(error) => {
            sender
                .send(Err(ModelError::Protocol(format!(
                    "invalid Ollama NDJSON chunk: {error}"
                ))))
                .await
                .map_err(|_| ())?;
            return Err(());
        }
    };
    if let Some(error) = chunk.error {
        sender
            .send(Err(ModelError::Protocol(format!(
                "Ollama error chunk: {error}"
            ))))
            .await
            .map_err(|_| ())?;
        return Err(());
    }
    if !chunk.message.content.is_empty() {
        sender
            .send(Ok(ModelEvent::Delta(chunk.message.content)))
            .await
            .map_err(|_| ())?;
    }
    let has_tool_calls = !chunk.message.tool_calls.is_empty();
    for tool_call in chunk.message.tool_calls {
        let arguments = match tool_call.function.arguments {
            Value::String(encoded) => match serde_json::from_str(&encoded) {
                Ok(value) => value,
                Err(error) => {
                    sender
                        .send(Err(ModelError::Protocol(format!(
                            "invalid Ollama tool arguments: {error}"
                        ))))
                        .await
                        .map_err(|_| ())?;
                    return Err(());
                }
            },
            value => value,
        };
        sender
            .send(Ok(ModelEvent::ToolCall(ToolInvocation {
                id: ToolCallId::new(),
                name: tool_call.function.name,
                arguments,
            })))
            .await
            .map_err(|_| ())?;
    }
    if chunk.done {
        sender
            .send(Ok(ModelEvent::Usage(TokenUsage {
                input: chunk.prompt_eval_count,
                output: chunk.eval_count,
                cache_read: 0,
            })))
            .await
            .map_err(|_| ())?;
        sender
            .send(Ok(ModelEvent::Done {
                stop_reason: if has_tool_calls {
                    StopReason::ToolUse
                } else if chunk.done_reason.as_deref() == Some("length") {
                    StopReason::MaxTokens
                } else {
                    StopReason::EndTurn
                },
            }))
            .await
            .map_err(|_| ())?;
    }
    Ok(())
}

fn trim_ascii_whitespace(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

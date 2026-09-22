use std::sync::Arc;

use agent_core::{
    Locality, ModelCapabilities, ModelTier, RiskLevel, StopReason, ToolDefinition, ToolSource,
};
use agent_model::{ChatMessage, ModelEvent, ModelProvider, ModelRequest, Role};
use futures::StreamExt;
use provider_ollama::OllamaProvider;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn capabilities(model: &str, supports_tools: bool) -> ModelCapabilities {
    ModelCapabilities {
        provider: "ollama".into(),
        model: model.into(),
        tier: if supports_tools {
            ModelTier::Worker
        } else {
            ModelTier::Control
        },
        locality: Locality::Local,
        context_window: 32_768,
        max_output_tokens: 2_048,
        supports_tools,
        supports_streaming: true,
        supports_json: true,
        supports_thinking: supports_tools,
        cost_per_mtok_in: 0.0,
        cost_per_mtok_out: 0.0,
    }
}

fn request(model: &str) -> ModelRequest {
    ModelRequest {
        model: model.into(),
        system: Some("Be concise".into()),
        messages: vec![ChatMessage {
            role: Role::User,
            content: "weather".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: vec![],
        }],
        tools: vec![ToolDefinition {
            name: "forecast".into(),
            description: "Get a forecast".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }),
            source: ToolSource::BuiltIn,
            risk: RiskLevel::Network,
            requires_secrets: vec![],
        }],
        max_tokens: 2_048,
        temperature: Some(0.2),
        json_mode: true,
        thinking: Some(false),
        stop: vec!["STOP".into()],
    }
}

#[tokio::test]
async fn sends_ollama_chat_options_tools_json_mode_and_think_false() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(body_json(json!({
            "model": "qwen3.8-27b:q3-claude",
            "messages": [
                {"role": "system", "content": "Be concise"},
                {"role": "user", "content": "weather"}
            ],
            "stream": true,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "forecast",
                    "description": "Get a forecast",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}},
                        "required": ["city"]
                    }
                }
            }],
            "format": "json",
            "think": false,
            "options": {
                "num_predict": 2048,
                "temperature": 0.2,
                "stop": ["STOP"]
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "{\"message\":{\"role\":\"assistant\",\"content\":\"ok\"},\"done\":false}\n\
             {\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":4,\"eval_count\":1}\n",
        ))
        .mount(&server)
        .await;

    let provider = OllamaProvider::new(
        server.uri(),
        vec![capabilities("qwen3.8-27b:q3-claude", true)],
    )
    .unwrap();
    let mut model_request = request("qwen3.8-27b:q3-claude");
    model_request.thinking = None;
    let stream = provider.stream(model_request).await.unwrap();
    let events = stream.events.collect::<Vec<_>>().await;

    assert!(events.iter().all(Result::is_ok));
}

#[tokio::test]
async fn parses_ndjson_deltas_tool_calls_usage_and_done() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "{\"message\":{\"role\":\"assistant\",\"content\":\"Let me \"},\"done\":false}\n\
             {\"message\":{\"role\":\"assistant\",\"content\":\"check.\",\"tool_calls\":[{\"function\":{\"name\":\"forecast\",\"arguments\":{\"city\":\"Lyon\"}}}]},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":12,\"eval_count\":7}\n",
        ))
        .mount(&server)
        .await;

    let provider = OllamaProvider::new(
        server.uri(),
        vec![capabilities("qwen3.8-27b:q3-claude", true)],
    )
    .unwrap();
    let events = provider
        .stream(request("qwen3.8-27b:q3-claude"))
        .await
        .unwrap()
        .events
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events[0], ModelEvent::Delta("Let me ".into()));
    assert_eq!(events[1], ModelEvent::Delta("check.".into()));
    let ModelEvent::ToolCall(call) = &events[2] else {
        panic!("expected tool call");
    };
    assert_eq!(call.name, "forecast");
    assert_eq!(call.arguments, json!({"city": "Lyon"}));
    assert_eq!(
        events[3],
        ModelEvent::Usage(agent_core::TokenUsage {
            input: 12,
            output: 7,
            cache_read: 0,
        })
    );
    assert_eq!(
        events[4],
        ModelEvent::Done {
            stop_reason: StopReason::ToolUse
        }
    );
}

#[tokio::test]
async fn rejects_requests_for_unconfigured_models_before_network_io() {
    let server = MockServer::start().await;
    let provider = Arc::new(
        OllamaProvider::new(server.uri(), vec![capabilities("translategemma:4b", false)]).unwrap(),
    );

    let error = provider
        .stream(request("missing-model"))
        .await
        .err()
        .expect("request should fail");

    assert!(error.to_string().contains("not configured"));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn returns_protocol_error_for_malformed_ndjson() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not-json\n"))
        .mount(&server)
        .await;
    let provider = OllamaProvider::new(
        server.uri(),
        vec![capabilities("qwen3.8-27b:q3-claude", true)],
    )
    .unwrap();

    let events = provider
        .stream(request("qwen3.8-27b:q3-claude"))
        .await
        .unwrap()
        .events
        .collect::<Vec<_>>()
        .await;

    assert_eq!(events.len(), 1);
    assert!(
        events[0]
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("NDJSON")
    );
}

#[tokio::test]
async fn returns_provider_error_chunk_and_stops_stream_processing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "{\"error\":\"model runner failed\"}\n\
             {\"message\":{\"role\":\"assistant\",\"content\":\"must not emit\"},\"done\":true}\n",
        ))
        .mount(&server)
        .await;
    let provider = OllamaProvider::new(
        server.uri(),
        vec![capabilities("qwen3.8-27b:q3-claude", true)],
    )
    .unwrap();

    let events = provider
        .stream(request("qwen3.8-27b:q3-claude"))
        .await
        .unwrap()
        .events
        .collect::<Vec<_>>()
        .await;

    assert_eq!(events.len(), 1);
    assert!(
        events[0]
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("model runner failed")
    );
}

#[test]
fn endpoint_rejects_userinfo() {
    let result = OllamaProvider::new(
        "https://user:password@example.com",
        vec![capabilities("model", false)],
    );

    let error = result.err().expect("userinfo must be rejected");
    assert!(error.to_string().contains("userinfo"));
}

#[test]
fn endpoint_requires_https_for_non_loopback_hosts() {
    for endpoint in ["http://example.com", "http://192.168.1.10:11434"] {
        let result = OllamaProvider::new(endpoint, vec![capabilities("model", false)]);
        let error = result
            .err()
            .expect("insecure remote endpoint must be rejected");
        assert!(error.to_string().contains("HTTPS"));
    }
}

#[test]
fn endpoint_allows_http_only_for_loopback_hosts() {
    for endpoint in [
        "http://localhost:11434",
        "http://127.42.0.1:11434",
        "http://[::1]:11434",
        "https://example.com",
    ] {
        OllamaProvider::new(endpoint, vec![capabilities("model", false)]).unwrap();
    }
}

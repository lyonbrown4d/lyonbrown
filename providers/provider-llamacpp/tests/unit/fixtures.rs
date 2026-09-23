    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use agent_core::{RiskLevel, ToolCallId, ToolDefinition, ToolSource};
    use agent_model::{ChatMessage, ModelRequest, Role};

    #[cfg(feature = "llama-cpp")]
    use super::StopBuffer;
    use super::{
        GgufModel, LlamaCppConfig, LlamaCppProvider, MAX_BATCH_SIZE, PromptMessage, ProviderError,
        acquire_inference_gate, capabilities, chunk_ranges, prompt_messages, try_admit_inference,
        validate_request,
    };
    use super::{Termination, termination_reason};
    use super::{
        MAX_BUFFERED_OUTPUT_BYTES, MAX_TOOL_CALL_BYTES, MAX_TOOL_GUIDANCE_BYTES,
        ParsedModelOutput, append_buffered_output, apply_qwen3_thinking_control,
        parse_model_output, tool_protocol_guidance,
    };
    #[cfg(feature = "llama-cpp")]
    use agent_model::{ModelEvent, ModelProvider};
    #[cfg(feature = "llama-cpp")]
    use tokio_stream::StreamExt;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct GgufFixture(PathBuf);

    impl GgufFixture {
        fn new() -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "lyonbrown-llamacpp-{}-{sequence}.gguf",
                std::process::id()
            ));
            fs::write(&path, b"not a real model").expect("create fixture");
            Self(path)
        }
    }

    impl Drop for GgufFixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn config(fixture: &GgufFixture) -> LlamaCppConfig {
        LlamaCppConfig {
            model: GgufModel::new("local-test", &fixture.0, 2_048, 256).expect("valid descriptor"),
            threads: 2,
            batch_threads: 2,
            batch_size: 512,
            gpu_layers: 0,
            temperature: 0.7,
            seed: 42,
        }
    }

    fn message(role: Role, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: content.into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: Vec::new(),
        }
    }

    fn request(model: &str) -> ModelRequest {
        ModelRequest {
            model: model.into(),
            system: Some("Be concise.".into()),
            messages: vec![
                message(Role::User, "hello"),
                message(Role::Assistant, "Hi!"),
                ChatMessage {
                    role: Role::Tool,
                    content: "tool output".into(),
                    tool_call_id: Some(ToolCallId::new()),
                    tool_name: Some("lookup".into()),
                    tool_calls: Vec::new(),
                },
            ],
            tools: Vec::new(),
            max_tokens: 64,
            temperature: None,
            json_mode: false,
            thinking: None,
            stop: vec!["STOP".into()],
        }
    }

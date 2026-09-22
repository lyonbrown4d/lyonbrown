//! Embedded, in-process llama.cpp model provider.
//!
//! The native binding is intentionally opt-in through the `llama-cpp` feature, so normal
//! workspace builds do not compile llama.cpp. Configuration and GGUF validation remain
//! available without the feature; constructing the provider then returns [`ProviderError::Unavailable`].

#[cfg(any(feature = "llama-cpp", test))]
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[cfg(any(feature = "llama-cpp", test))]
use agent_core::{Locality, ModelCapabilities, ModelTier};
use agent_model::ModelError;
#[cfg(any(feature = "llama-cpp", test))]
use agent_model::{ModelRequest, Role};

#[cfg(feature = "llama-cpp")]
use std::num::NonZeroU32;
#[cfg(feature = "llama-cpp")]
use std::sync::Arc;

#[cfg(any(feature = "llama-cpp", test))]
use agent_core::StopReason;
#[cfg(feature = "llama-cpp")]
use agent_core::TokenUsage;
#[cfg(feature = "llama-cpp")]
use agent_model::{ModelEvent, ModelProvider, ModelStream};
#[cfg(feature = "llama-cpp")]
use async_trait::async_trait;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::context::params::LlamaContextParams;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::llama_backend::LlamaBackend;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::llama_batch::LlamaBatch;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::model::params::LlamaModelParams;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::model::{AddBos, LlamaModel};
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::sampling::LlamaSampler;
#[cfg(feature = "llama-cpp")]
use tokio::sync::{Mutex, mpsc};
#[cfg(feature = "llama-cpp")]
use tokio_stream::wrappers::ReceiverStream;

/// A validated local GGUF model descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufModel {
    id: String,
    path: PathBuf,
    context_size: u32,
    max_output_tokens: u32,
}

impl GgufModel {
    /// Validates and canonicalizes a model file.
    pub fn new(
        id: impl Into<String>,
        path: impl AsRef<Path>,
        context_size: u32,
        max_output_tokens: u32,
    ) -> Result<Self, ProviderError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(ProviderError::InvalidConfig(
                "model id must not be empty".into(),
            ));
        }
        if path.as_ref().extension().and_then(|value| value.to_str()) != Some("gguf") {
            return Err(ProviderError::InvalidConfig(
                "model path must have the .gguf extension".into(),
            ));
        }
        let path = path.as_ref().canonicalize().map_err(|error| {
            ProviderError::InvalidConfig(format!("cannot resolve model path: {error}"))
        })?;
        if !path.is_file() {
            return Err(ProviderError::InvalidConfig(
                "model path must be a regular file".into(),
            ));
        }
        if context_size == 0 || context_size > i32::MAX as u32 {
            return Err(ProviderError::InvalidConfig(
                "context size must be between 1 and i32::MAX".into(),
            ));
        }
        if max_output_tokens == 0 || max_output_tokens > context_size {
            return Err(ProviderError::InvalidConfig(
                "max output tokens must be between 1 and the context size".into(),
            ));
        }
        Ok(Self {
            id,
            path,
            context_size,
            max_output_tokens,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn context_size(&self) -> u32 {
        self.context_size
    }

    pub fn max_output_tokens(&self) -> u32 {
        self.max_output_tokens
    }
}

/// Runtime settings for one embedded llama.cpp model.
#[derive(Debug, Clone, PartialEq)]
pub struct LlamaCppConfig {
    pub model: GgufModel,
    pub threads: u32,
    pub batch_threads: u32,
    pub gpu_layers: u32,
    pub temperature: f32,
    pub seed: u32,
}

impl LlamaCppConfig {
    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.threads == 0 || self.threads > i32::MAX as u32 {
            return Err(ProviderError::InvalidConfig(
                "threads must be between 1 and i32::MAX".into(),
            ));
        }
        if self.batch_threads == 0 || self.batch_threads > i32::MAX as u32 {
            return Err(ProviderError::InvalidConfig(
                "batch threads must be between 1 and i32::MAX".into(),
            ));
        }
        if self.gpu_layers > i32::MAX as u32 {
            return Err(ProviderError::InvalidConfig(
                "GPU layers must not exceed i32::MAX".into(),
            ));
        }
        if !self.temperature.is_finite() || self.temperature < 0.0 {
            return Err(ProviderError::InvalidConfig(
                "temperature must be finite and non-negative".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("invalid llama.cpp configuration: {0}")]
    InvalidConfig(String),
    #[error("invalid llama.cpp request: {0}")]
    InvalidRequest(String),
    #[error("embedded llama.cpp support is unavailable; enable the `llama-cpp` feature")]
    Unavailable,
    #[error("llama.cpp failure: {0}")]
    Native(String),
}

impl From<ProviderError> for ModelError {
    fn from(error: ProviderError) -> Self {
        Self::Other(error.to_string())
    }
}

#[cfg(any(feature = "llama-cpp", test))]
fn capabilities(config: &LlamaCppConfig) -> ModelCapabilities {
    ModelCapabilities {
        provider: "llamacpp".into(),
        model: config.model.id.clone(),
        tier: ModelTier::Worker,
        locality: Locality::Local,
        context_window: config.model.context_size,
        max_output_tokens: config.model.max_output_tokens,
        supports_tools: false,
        supports_streaming: true,
        supports_json: false,
        supports_thinking: false,
        cost_per_mtok_in: 0.0,
        cost_per_mtok_out: 0.0,
    }
}

#[cfg(any(feature = "llama-cpp", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Termination {
    Eog,
    StopString,
    OutputLimit,
}

#[cfg(any(feature = "llama-cpp", test))]
fn termination_reason(termination: Termination) -> StopReason {
    match termination {
        Termination::Eog => StopReason::EndTurn,
        Termination::StopString => StopReason::Stop,
        Termination::OutputLimit => StopReason::MaxTokens,
    }
}

/// A single-model embedded llama.cpp provider.
pub struct LlamaCppProvider {
    config: LlamaCppConfig,
    #[cfg(feature = "llama-cpp")]
    native: Arc<NativeState>,
    #[cfg(feature = "llama-cpp")]
    inference_gate: Arc<Mutex<()>>,
}

impl std::fmt::Debug for LlamaCppProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LlamaCppProvider")
            .field("model", &self.config.model.id)
            .finish_non_exhaustive()
    }
}

#[cfg(not(feature = "llama-cpp"))]
impl LlamaCppProvider {
    pub fn new(config: LlamaCppConfig) -> Result<Self, ProviderError> {
        config.validate()?;
        Err(ProviderError::Unavailable)
    }
}

#[cfg(feature = "llama-cpp")]
struct NativeState {
    // Fields drop in declaration order: the model must be freed before its backend.
    model: LlamaModel,
    backend: LlamaBackend,
}

#[cfg(feature = "llama-cpp")]
impl LlamaCppProvider {
    /// Initializes llama.cpp and loads the configured GGUF once.
    pub fn new(config: LlamaCppConfig) -> Result<Self, ProviderError> {
        config.validate()?;
        let model_path = config.model.path.clone();
        let gpu_layers = config.gpu_layers;
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut backend = LlamaBackend::init().map_err(native_error)?;
            backend.void_logs();
            if gpu_layers > 0 && !backend.supports_gpu_offload() {
                return Err(ProviderError::InvalidConfig(
                    "GPU layers requested, but this build has no supported GPU backend".into(),
                ));
            }
            let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
            let model =
                LlamaModel::load_from_file(&backend, &model_path, &params).map_err(native_error)?;
            Ok(NativeState { model, backend })
        }));
        let native = match loaded {
            Ok(result) => result?,
            Err(_) => {
                return Err(ProviderError::Native(
                    "native initialization panicked".into(),
                ));
            }
        };
        Ok(Self {
            config,
            native: Arc::new(native),
            inference_gate: Arc::new(Mutex::new(())),
        })
    }
}

#[cfg(feature = "llama-cpp")]
fn native_error(error: impl std::fmt::Display) -> ProviderError {
    ProviderError::Native(error.to_string())
}

#[cfg(feature = "llama-cpp")]
#[async_trait]
impl ModelProvider for LlamaCppProvider {
    fn provider_id(&self) -> &'static str {
        "llamacpp"
    }

    async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
        Ok(vec![capabilities(&self.config)])
    }

    async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
        validate_request(&self.config, &request)?;

        let model_id = self.config.model.id.clone();
        let prompt = format_prompt(&request);
        let max_tokens = request.max_tokens.min(self.config.model.max_output_tokens);
        let temperature = request.temperature.unwrap_or(self.config.temperature);
        let stops = request.stop;

        let native = Arc::clone(&self.native);
        let gate = Arc::clone(&self.inference_gate);
        let config = self.config.clone();
        let (sender, receiver) = mpsc::channel(8);
        let error_sender = sender.clone();
        tokio::spawn(async move {
            let guard = gate.lock_owned().await;
            if sender.is_closed() {
                return;
            }
            let result = tokio::task::spawn_blocking(move || {
                let _guard = guard;
                generate(
                    &native,
                    &config,
                    &prompt,
                    max_tokens,
                    temperature,
                    &stops,
                    &sender,
                )
            })
            .await;

            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(ModelError::Other(format!(
                    "llama.cpp inference task failed: {error}"
                ))),
            };
            if let Some(error) = error {
                let _ = error_sender.send(Err(error)).await;
            }
        });

        Ok(ModelStream {
            model: model_id,
            events: Box::pin(ReceiverStream::new(receiver)),
        })
    }
}

#[cfg(feature = "llama-cpp")]
fn generate(
    native: &NativeState,
    config: &LlamaCppConfig,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
    stops: &[String],
    sender: &mpsc::Sender<Result<ModelEvent, ModelError>>,
) -> Result<(), ModelError> {
    let context_size = NonZeroU32::new(config.model.context_size)
        .ok_or_else(|| ModelError::Other("context size is zero".into()))?;
    let context_params = LlamaContextParams::default()
        .with_n_ctx(Some(context_size))
        .with_n_batch(config.model.context_size)
        .with_n_threads(config.threads as i32)
        .with_n_threads_batch(config.batch_threads as i32);
    let mut context = native
        .model
        .new_context(&native.backend, context_params)
        .map_err(|error| ModelError::Other(format!("cannot create llama.cpp context: {error}")))?;
    let prompt_tokens = native
        .model
        .str_to_token(prompt, AddBos::Always)
        .map_err(|error| ModelError::Other(format!("cannot tokenize prompt: {error}")))?;
    let prompt_len = u32::try_from(prompt_tokens.len())
        .map_err(|_| ModelError::Other("prompt token count exceeds u32".into()))?;
    if prompt_tokens.is_empty() || prompt_len >= config.model.context_size {
        return Err(ModelError::Other(
            "prompt does not fit in the configured context".into(),
        ));
    }
    let output_limit = max_tokens.min(config.model.context_size - prompt_len);
    let mut batch = LlamaBatch::new(prompt_tokens.len(), 1);
    batch
        .add_sequence(&prompt_tokens, 0, false)
        .map_err(|error| ModelError::Other(format!("cannot create prompt batch: {error}")))?;
    context
        .decode(&mut batch)
        .map_err(|error| ModelError::Other(format!("cannot decode prompt: {error}")))?;

    let mut sampler = if temperature == 0.0 {
        LlamaSampler::greedy()
    } else {
        LlamaSampler::chain_simple([
            LlamaSampler::temp(temperature),
            LlamaSampler::dist(config.seed),
        ])
    };
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut stop_buffer = StopBuffer::new(stops);
    let mut generated = 0_u32;
    let mut termination = Termination::OutputLimit;

    while generated < output_limit {
        if sender.is_closed() {
            return Ok(());
        }
        let token = sampler.sample(&context, -1);
        if native.model.is_eog_token(token) {
            termination = Termination::Eog;
            break;
        }
        generated += 1;
        let piece = native
            .model
            .token_to_piece(token, &mut decoder, false, None)
            .map_err(|error| ModelError::Other(format!("cannot decode token: {error}")))?;
        let (delta, stopped) = stop_buffer.push(&piece);
        if let Some(delta) = delta
            && sender.blocking_send(Ok(ModelEvent::Delta(delta))).is_err()
        {
            return Ok(());
        }
        if stopped {
            termination = Termination::StopString;
            break;
        }

        batch.clear();
        let position = i32::try_from(prompt_len + generated - 1)
            .map_err(|_| ModelError::Other("token position exceeds i32".into()))?;
        batch
            .add(token, position, &[0], true)
            .map_err(|error| ModelError::Other(format!("cannot add generated token: {error}")))?;
        context.decode(&mut batch).map_err(|error| {
            ModelError::Other(format!("cannot decode generated token: {error}"))
        })?;
    }

    if let Some(delta) = stop_buffer.finish()
        && sender.blocking_send(Ok(ModelEvent::Delta(delta))).is_err()
    {
        return Ok(());
    }
    let usage = TokenUsage {
        input: prompt_len,
        output: generated,
        cache_read: 0,
    };
    if sender.blocking_send(Ok(ModelEvent::Usage(usage))).is_err() {
        return Ok(());
    }
    let _ = sender.blocking_send(Ok(ModelEvent::Done {
        stop_reason: termination_reason(termination),
    }));
    Ok(())
}

#[cfg(feature = "llama-cpp")]
struct StopBuffer<'a> {
    stops: &'a [String],
    pending: String,
    stopped: bool,
}

#[cfg(feature = "llama-cpp")]
impl<'a> StopBuffer<'a> {
    fn new(stops: &'a [String]) -> Self {
        Self {
            stops,
            pending: String::new(),
            stopped: false,
        }
    }

    fn push(&mut self, piece: &str) -> (Option<String>, bool) {
        self.pending.push_str(piece);
        if let Some(index) = self
            .stops
            .iter()
            .filter_map(|stop| self.pending.find(stop))
            .min()
        {
            let delta = self.pending[..index].to_owned();
            self.pending.clear();
            self.stopped = true;
            return ((!delta.is_empty()).then_some(delta), true);
        }

        let retained = self
            .pending
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(self.pending.len()))
            .filter_map(|index| {
                let suffix = &self.pending[index..];
                self.stops
                    .iter()
                    .any(|stop| stop.starts_with(suffix))
                    .then_some(suffix.len())
            })
            .max()
            .unwrap_or(0);
        let emit_len = self.pending.len() - retained;
        let held = self.pending.split_off(emit_len);
        let emitted = std::mem::replace(&mut self.pending, held);
        ((!emitted.is_empty()).then_some(emitted), false)
    }

    fn finish(&mut self) -> Option<String> {
        if self.stopped || self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }
}

#[cfg(any(feature = "llama-cpp", test))]
fn validate_request(config: &LlamaCppConfig, request: &ModelRequest) -> Result<(), ProviderError> {
    if request.model != config.model.id {
        return Err(ProviderError::InvalidRequest(format!(
            "unknown model id {:?}",
            request.model
        )));
    }
    if !request.tools.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "tool calling is not supported".into(),
        ));
    }
    if request.json_mode {
        return Err(ProviderError::InvalidRequest(
            "JSON mode is not supported".into(),
        ));
    }
    if request.thinking == Some(true) {
        return Err(ProviderError::InvalidRequest(
            "thinking mode is not supported".into(),
        ));
    }
    if request.max_tokens == 0 {
        return Err(ProviderError::InvalidRequest(
            "max_tokens must be nonzero".into(),
        ));
    }
    if request
        .temperature
        .is_some_and(|temperature| !temperature.is_finite() || temperature < 0.0)
    {
        return Err(ProviderError::InvalidRequest(
            "temperature must be finite and non-negative".into(),
        ));
    }
    if request.stop.iter().any(String::is_empty) {
        return Err(ProviderError::InvalidRequest(
            "stop strings must not be empty".into(),
        ));
    }
    Ok(())
}

#[cfg(any(feature = "llama-cpp", test))]
fn format_prompt(request: &ModelRequest) -> String {
    let mut prompt = String::new();
    if let Some(system) = &request.system {
        let _ = write!(prompt, "<|system|>\n{system}\n");
    }
    for message in &request.messages {
        match message.role {
            Role::System => {
                let _ = write!(prompt, "<|system|>\n{}\n", message.content);
            }
            Role::User => {
                let _ = write!(prompt, "<|user|>\n{}\n", message.content);
            }
            Role::Assistant => {
                let _ = write!(prompt, "<|assistant|>\n{}\n", message.content);
            }
            Role::Tool => {
                let name = message.tool_name.as_deref().unwrap_or("unknown");
                let id = message
                    .tool_call_id
                    .map_or_else(|| "unknown".into(), |id| id.to_string());
                let _ = write!(prompt, "<|tool:{name}:{id}|>\n{}\n", message.content);
            }
        }
    }
    prompt.push_str("<|assistant|>\n");
    prompt
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use agent_core::{RiskLevel, ToolCallId, ToolDefinition, ToolSource};
    use agent_model::{ChatMessage, ModelRequest, Role};

    #[cfg(feature = "llama-cpp")]
    use super::StopBuffer;
    use super::{
        GgufModel, LlamaCppConfig, LlamaCppProvider, ProviderError, capabilities, format_prompt,
        validate_request,
    };
    use super::{Termination, termination_reason};
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

    #[test]
    fn descriptor_canonicalizes_a_regular_gguf_file() {
        let fixture = GgufFixture::new();
        let model = GgufModel::new("model", &fixture.0, 1_024, 64).expect("descriptor");

        assert!(model.path().is_absolute());
        assert_eq!(
            model.path(),
            fixture.0.canonicalize().expect("canonical path")
        );
    }

    #[test]
    fn descriptor_rejects_invalid_identity_path_and_limits() {
        let fixture = GgufFixture::new();
        let directory = fixture.0.parent().expect("fixture parent");

        assert!(GgufModel::new(" ", &fixture.0, 1, 1).is_err());
        assert!(GgufModel::new("model", directory, 1, 1).is_err());
        assert!(GgufModel::new("model", directory.join("missing.gguf"), 1, 1).is_err());
        assert!(GgufModel::new("model", fixture.0.with_extension("bin"), 1, 1).is_err());
        assert!(GgufModel::new("model", &fixture.0, 0, 1).is_err());
        assert!(GgufModel::new("model", &fixture.0, 1, 0).is_err());
        assert!(GgufModel::new("model", &fixture.0, 16, 17).is_err());
    }

    #[test]
    fn config_rejects_invalid_runtime_values() {
        let fixture = GgufFixture::new();
        let mut candidate = config(&fixture);
        candidate.threads = 0;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.batch_threads = 0;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.temperature = f32::NAN;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.temperature = -0.1;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.gpu_layers = i32::MAX as u32 + 1;
        assert!(candidate.validate().is_err());
    }

    #[test]
    fn prompt_format_is_deterministic_for_all_message_roles() {
        let request = request("local-test");
        let tool_call_id = request.messages[2].tool_call_id.expect("tool call id");
        let prompt = format_prompt(&request);

        assert_eq!(
            prompt,
            format!(
                "<|system|>\nBe concise.\n<|user|>\nhello\n<|assistant|>\nHi!\n<|tool:lookup:{tool_call_id}|>\ntool output\n<|assistant|>\n"
            )
        );
    }

    #[test]
    fn request_validation_rejects_unsupported_modes_and_wrong_model() {
        let fixture = GgufFixture::new();
        let config = config(&fixture);

        let error = validate_request(&config, &request("other")).expect_err("wrong model");
        assert!(matches!(error, ProviderError::InvalidRequest(_)));

        let mut with_tools = request("local-test");
        with_tools.tools.push(ToolDefinition {
            name: "lookup".into(),
            description: "lookup".into(),
            input_schema: serde_json::json!({"type": "object"}),
            source: ToolSource::BuiltIn,
            risk: RiskLevel::Read,
            requires_secrets: Vec::new(),
        });
        assert!(validate_request(&config, &with_tools).is_err());

        let mut json = request("local-test");
        json.json_mode = true;
        assert!(validate_request(&config, &json).is_err());

        let mut thinking = request("local-test");
        thinking.thinking = Some(true);
        assert!(validate_request(&config, &thinking).is_err());

        let mut empty_stop = request("local-test");
        empty_stop.stop = vec![String::new()];
        assert!(validate_request(&config, &empty_stop).is_err());
    }

    #[test]
    fn capability_reports_one_local_streaming_worker() {
        let fixture = GgufFixture::new();
        let capability = capabilities(&config(&fixture));

        assert_eq!(capability.provider, "llamacpp");
        assert_eq!(capability.model, "local-test");
        assert_eq!(capability.context_window, 2_048);
        assert_eq!(capability.max_output_tokens, 256);
        assert!(capability.supports_streaming);
        assert!(!capability.supports_tools);
        assert!(!capability.supports_json);
        assert!(!capability.supports_thinking);
    }

    #[test]
    fn natural_eog_ends_the_turn() {
        assert_eq!(
            termination_reason(Termination::Eog),
            agent_core::StopReason::EndTurn
        );
    }

    #[test]
    fn configured_stop_reports_stop() {
        assert_eq!(
            termination_reason(Termination::StopString),
            agent_core::StopReason::Stop
        );
    }

    #[test]
    fn exhausted_output_budget_reports_max_tokens() {
        assert_eq!(
            termination_reason(Termination::OutputLimit),
            agent_core::StopReason::MaxTokens
        );
    }

    #[cfg(not(feature = "llama-cpp"))]
    #[test]
    fn constructor_is_explicitly_unavailable_without_native_feature() {
        let fixture = GgufFixture::new();
        let error = LlamaCppProvider::new(config(&fixture)).expect_err("feature is disabled");

        assert!(matches!(error, ProviderError::Unavailable));
    }

    #[cfg(feature = "llama-cpp")]
    #[test]
    fn invalid_gguf_fails_cleanly_during_construction() {
        let fixture = GgufFixture::new();
        let error = LlamaCppProvider::new(config(&fixture)).expect_err("invalid GGUF");

        assert!(matches!(error, ProviderError::Native(_)));
    }

    #[cfg(feature = "llama-cpp")]
    #[test]
    fn stop_buffer_handles_a_stop_string_split_across_tokens() {
        let stops = ["STOP".into()];
        let mut buffer = StopBuffer::new(&stops);

        assert_eq!(buffer.push("answer ST"), (Some("answer ".into()), false));
        assert_eq!(buffer.push("OP ignored"), (None, true));
        assert_eq!(buffer.finish(), None);
    }

    #[cfg(feature = "llama-cpp")]
    #[tokio::test]
    #[ignore = "requires a real GGUF path in LYONBROWN_GGUF_MODEL"]
    async fn live_completion_smoke_test() {
        let path = std::env::var_os("LYONBROWN_GGUF_MODEL")
            .expect("set LYONBROWN_GGUF_MODEL to a local GGUF file");
        let config = LlamaCppConfig {
            model: GgufModel::new("live", path, 2_048, 32).expect("valid model descriptor"),
            threads: 2,
            batch_threads: 2,
            gpu_layers: 0,
            temperature: 0.0,
            seed: 42,
        };
        let provider = LlamaCppProvider::new(config).expect("load live model");
        let mut stream = provider
            .stream(ModelRequest {
                model: "live".into(),
                system: None,
                messages: vec![message(Role::User, "Reply with one short greeting.")],
                tools: Vec::new(),
                max_tokens: 16,
                temperature: Some(0.0),
                json_mode: false,
                thinking: None,
                stop: Vec::new(),
            })
            .await
            .expect("start completion");
        let mut saw_delta = false;
        let mut saw_done = false;
        while let Some(event) = stream.events.next().await {
            match event.expect("completion event") {
                ModelEvent::Delta(delta) => saw_delta |= !delta.is_empty(),
                ModelEvent::Done { .. } => saw_done = true,
                ModelEvent::Usage(_) | ModelEvent::ToolCall(_) => {}
            }
        }
        assert!(saw_delta, "model returned no text");
        assert!(saw_done, "model stream did not finish");
    }
}

use crate::{
  FinishReason, GenerationRequest, GenerationSummary, InferenceBackend, InferenceChunk,
  InferenceError, InputContent, MessageRole, ModelInfo, Result,
};
use async_trait::async_trait;
use llama_cpp_2::{
  context::params::LlamaContextParams,
  llama_backend::LlamaBackend,
  llama_batch::LlamaBatch,
  model::{AddBos, LlamaChatMessage, LlamaModel, params::LlamaModelParams},
  sampling::LlamaSampler,
};
use std::{
  num::NonZeroU32,
  path::Path,
  sync::{Arc, OnceLock},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
static BACKEND: OnceLock<std::result::Result<Arc<LlamaBackend>, String>> = OnceLock::new();

pub struct LlamaCppBackend {
  model: Arc<LlamaModel>,
  backend: Arc<LlamaBackend>,
  info: ModelInfo,
  gpu_offload: bool,
}
impl LlamaCppBackend {
  /// Load an existing GGUF. Model files are never downloaded by this adapter.
  pub async fn load(
    id: String,
    path: impl AsRef<Path>,
    context_window: u32,
    gpu_layers: u32,
  ) -> Result<Self> {
    if context_window == 0 || context_window > i32::MAX as u32 {
      return Err(InferenceError::InvalidRequest(
        "invalid llama context size".into(),
      ));
    }
    if gpu_layers > 0 && !cfg!(any(feature = "llama-metal", feature = "llama-cuda")) {
      return Err(InferenceError::InvalidRequest(
        "GPU offload was not compiled".into(),
      ));
    }
    let path = std::fs::canonicalize(path).map_err(backend_error)?;
    tokio::task::spawn_blocking(move || {
      let backend = BACKEND
        .get_or_init(|| {
          llama_cpp_2::send_logs_to_tracing(
            llama_cpp_2::LogOptions::default().with_logs_enabled(false),
          );
          LlamaBackend::init()
            .map(Arc::new)
            .map_err(|error| error.to_string())
        })
        .as_ref()
        .map(Arc::clone)
        .map_err(|error| InferenceError::Backend(error.clone()))?;
      let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
      let model = LlamaModel::load_from_file(&backend, path, &params).map_err(backend_error)?;
      model.chat_template(None).map_err(backend_error)?;
      Ok(Self {
        gpu_offload: gpu_layers > 0,
        model: Arc::new(model),
        backend,
        info: ModelInfo {
          id,
          backend: "llama-cpp-2".into(),
          context_window,
          supports_tools: false,
          supports_images: false,
        },
      })
    })
    .await
    .map_err(backend_error)?
  }
}
#[async_trait]
impl InferenceBackend for LlamaCppBackend {
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
    let model = self.model.clone();
    let backend = self.backend.clone();
    let context_window = self.info.context_window;
    let gpu_offload = self.gpu_offload;
    tokio::task::spawn_blocking(move || {
      generate(
        model,
        backend,
        context_window,
        gpu_offload,
        request,
        cancellation,
        output,
      )
    })
    .await
    .map_err(backend_error)?
  }
}
fn generate(
  model: Arc<LlamaModel>,
  backend: Arc<LlamaBackend>,
  context_window: u32,
  gpu_offload: bool,
  request: GenerationRequest,
  cancellation: CancellationToken,
  output: mpsc::Sender<InferenceChunk>,
) -> Result<GenerationSummary> {
  let chat = request
    .messages
    .into_iter()
    .map(|message| {
      let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
      };
      let text = message
        .content
        .into_iter()
        .filter_map(|part| match part {
          InputContent::Text { text } => Some(text),
          _ => None,
        })
        .collect::<String>();
      LlamaChatMessage::new(role.into(), text).map_err(backend_error)
    })
    .collect::<Result<Vec<_>>>()?;
  let template = model.chat_template(None).map_err(backend_error)?;
  let prompt = model
    .apply_chat_template(&template, &chat, true)
    .map_err(backend_error)?;
  let tokens = model
    .str_to_token(&prompt, AddBos::Always)
    .map_err(backend_error)?;
  let input = tokens.len();
  if input == 0 || input + request.max_tokens as usize > context_window as usize {
    return Err(InferenceError::InvalidRequest(
      "prompt and output exceed loaded context window".into(),
    ));
  }
  let params = LlamaContextParams::default()
    .with_n_ctx(NonZeroU32::new(context_window))
    .with_n_batch(512)
    .with_offload_kqv(gpu_offload)
    .with_op_offload(gpu_offload);
  let mut context = model.new_context(&backend, params).map_err(backend_error)?;
  let mut batch = LlamaBatch::new(512, 1);
  for (chunk_index, chunk) in tokens.chunks(512).enumerate() {
    if cancellation.is_cancelled() {
      return Err(InferenceError::Cancelled);
    }
    batch.clear();
    for (index, token) in chunk.iter().enumerate() {
      let position = chunk_index * 512 + index;
      batch
        .add(
          *token,
          i32::try_from(position).map_err(backend_error)?,
          &[0],
          position + 1 == input,
        )
        .map_err(backend_error)?;
    }
    context.decode(&mut batch).map_err(backend_error)?;
  }
  let mut sampler = if request.temperature.unwrap_or(0.0) == 0.0 {
    LlamaSampler::greedy()
  } else {
    LlamaSampler::chain_simple([
      LlamaSampler::temp(request.temperature.unwrap_or(1.0) as f32),
      LlamaSampler::dist(u32::try_from(request.seed).map_err(backend_error)?),
    ])
  };
  let mut decoder = encoding_rs::UTF_8.new_decoder();
  let mut generated = 0;
  let mut reason = FinishReason::Length;
  let runtime = tokio::runtime::Handle::current();
  for offset in 0..request.max_tokens {
    if cancellation.is_cancelled() {
      return Err(InferenceError::Cancelled);
    }
    let token = sampler.sample(&context, batch.n_tokens() - 1);
    sampler.accept(token);
    if model.is_eog_token(token) {
      reason = FinishReason::Stop;
      break;
    }
    let text = model
      .token_to_piece(token, &mut decoder, false, None)
      .map_err(backend_error)?;
    runtime.block_on(crate::contracts::emit(
      &output,
      &cancellation,
      InferenceChunk::Text { text },
    ))?;
    generated += 1;
    batch.clear();
    batch
      .add(
        token,
        i32::try_from(input + offset as usize).map_err(backend_error)?,
        &[0],
        true,
      )
      .map_err(backend_error)?;
    context.decode(&mut batch).map_err(backend_error)?;
  }
  Ok(GenerationSummary {
    reason,
    input_tokens: input as u64,
    output_tokens: generated,
  })
}
fn backend_error(error: impl std::fmt::Display) -> InferenceError {
  InferenceError::Backend(error.to_string())
}

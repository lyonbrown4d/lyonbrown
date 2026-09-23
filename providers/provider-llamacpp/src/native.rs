use super::*;

/// A single-model embedded llama.cpp provider.
pub struct LlamaCppProvider {
  config: LlamaCppConfig,
  #[cfg(feature = "llama-cpp")]
  native: Arc<NativeState>,
  #[cfg(feature = "llama-cpp")]
  inference_gate: Arc<Mutex<()>>,
  #[cfg(feature = "llama-cpp")]
  admission: Arc<Semaphore>,
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
      admission: Arc::new(Semaphore::new(1)),
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
    let admission_permit = try_admit_inference(Arc::clone(&self.admission))?;

    let model_id = self.config.model.id.clone();
    let max_tokens = request.max_tokens.min(self.config.model.max_output_tokens);
    let temperature = request.temperature.unwrap_or(self.config.temperature);
    let stops = request.stop.clone();

    let native = Arc::clone(&self.native);
    let gate = Arc::clone(&self.inference_gate);
    let config = self.config.clone();
    let (sender, receiver) = mpsc::channel(8);
    let error_sender = sender.clone();
    tokio::spawn(async move {
      let _admission_permit = admission_permit;
      let Some(guard) = acquire_inference_gate(gate, sender.clone()).await else {
        return;
      };
      if sender.is_closed() {
        return;
      }
      let result = tokio::task::spawn_blocking(move || {
        let _guard = guard;
        generate(
          &native,
          &config,
          &request,
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
  request: &ModelRequest,
  max_tokens: u32,
  temperature: f32,
  stops: &[String],
  sender: &mpsc::Sender<Result<ModelEvent, ModelError>>,
) -> Result<(), ModelError> {
  if sender.is_closed() {
    return Ok(());
  }
  let prompt = render_prompt(&native.model, request)?;
  if sender.is_closed() {
    return Ok(());
  }
  let prompt_tokens = native
    .model
    .str_to_token(&prompt, AddBos::Never)
    .map_err(|error| ModelError::Other(format!("cannot tokenize prompt: {error}")))?;
  if sender.is_closed() {
    return Ok(());
  }
  let prompt_len = u32::try_from(prompt_tokens.len())
    .map_err(|_| ModelError::Other("prompt token count exceeds u32".into()))?;
  if prompt_tokens.is_empty() || prompt_len >= config.model.context_size {
    return Err(ModelError::Other(
      "prompt does not fit in the configured context".into(),
    ));
  }

  let context_size = NonZeroU32::new(config.model.context_size)
    .ok_or_else(|| ModelError::Other("context size is zero".into()))?;
  let context_params = LlamaContextParams::default()
    .with_n_ctx(Some(context_size))
    .with_n_batch(config.batch_size)
    .with_n_threads(config.threads as i32)
    .with_n_threads_batch(config.batch_threads as i32);
  let mut context = native
    .model
    .new_context(&native.backend, context_params)
    .map_err(|error| ModelError::Other(format!("cannot create llama.cpp context: {error}")))?;
  if sender.is_closed() {
    return Ok(());
  }
  let output_limit = max_tokens.min(config.model.context_size - prompt_len);
  let batch_size = usize::try_from(config.batch_size)
    .map_err(|_| ModelError::Other("batch size exceeds usize".into()))?;
  let mut batch = LlamaBatch::new(batch_size, 1);
  for range in chunk_ranges(prompt_tokens.len(), batch_size) {
    if sender.is_closed() {
      return Ok(());
    }
    batch.clear();
    for index in range {
      let position = i32::try_from(index)
        .map_err(|_| ModelError::Other("prompt token position exceeds i32".into()))?;
      batch
        .add(
          prompt_tokens[index],
          position,
          &[0],
          index + 1 == prompt_tokens.len(),
        )
        .map_err(|error| ModelError::Other(format!("cannot create prompt batch: {error}")))?;
    }
    context
      .decode(&mut batch)
      .map_err(|error| ModelError::Other(format!("cannot decode prompt: {error}")))?;
    if sender.is_closed() {
      return Ok(());
    }
  }

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
  let buffer_output = !request.tools.is_empty();
  let mut buffered_output = String::new();
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
    if let Some(delta) = delta {
      if buffer_output {
        append_buffered_output(&mut buffered_output, &delta)?;
      } else if sender.blocking_send(Ok(ModelEvent::Delta(delta))).is_err() {
        return Ok(());
      }
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
    context
      .decode(&mut batch)
      .map_err(|error| ModelError::Other(format!("cannot decode generated token: {error}")))?;
  }

  if let Some(delta) = stop_buffer.finish() {
    if buffer_output {
      append_buffered_output(&mut buffered_output, &delta)?;
    } else if sender.blocking_send(Ok(ModelEvent::Delta(delta))).is_err() {
      return Ok(());
    }
  }
  let mut stop_reason = termination_reason(termination);
  if buffer_output {
    let Some(reason) = emit_buffered_output(&buffered_output, &request.tools, sender, stop_reason)?
    else {
      return Ok(());
    };
    stop_reason = reason;
  }
  let usage = TokenUsage {
    input: prompt_len,
    output: generated,
    cache_read: 0,
  };
  if sender.blocking_send(Ok(ModelEvent::Usage(usage))).is_err() {
    return Ok(());
  }
  let _ = sender.blocking_send(Ok(ModelEvent::Done { stop_reason }));
  Ok(())
}

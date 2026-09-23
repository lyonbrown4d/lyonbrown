use super::*;

#[cfg(any(feature = "llama-cpp", test))]
pub(super) fn capabilities(config: &LlamaCppConfig) -> ModelCapabilities {
  ModelCapabilities {
    provider: "llamacpp".into(),
    model: config.model.id.clone(),
    tier: ModelTier::Worker,
    locality: Locality::Local,
    context_window: config.model.context_size,
    max_output_tokens: config.model.max_output_tokens,
    supports_tools: true,
    supports_streaming: true,
    supports_json: false,
    supports_thinking: false,
    cost_per_mtok_in: 0.0,
    cost_per_mtok_out: 0.0,
  }
}

#[cfg(any(feature = "llama-cpp", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Termination {
  Eog,
  StopString,
  OutputLimit,
}

#[cfg(any(feature = "llama-cpp", test))]
pub(super) fn termination_reason(termination: Termination) -> StopReason {
  match termination {
    Termination::Eog => StopReason::EndTurn,
    Termination::StopString => StopReason::Stop,
    Termination::OutputLimit => StopReason::MaxTokens,
  }
}

#[cfg(any(feature = "llama-cpp", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PromptMessage {
  pub(super) role: String,
  pub(super) content: String,
}

#[cfg(any(feature = "llama-cpp", test))]
impl PromptMessage {
  pub(super) fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
    Self {
      role: role.into(),
      content: content.into(),
    }
  }
}

#[cfg(any(feature = "llama-cpp", test))]
pub(super) fn prompt_messages(request: &ModelRequest) -> Result<Vec<PromptMessage>, ModelError> {
  let mut messages =
    Vec::with_capacity(request.messages.len() + usize::from(request.system.is_some()));
  let mut system = request.system.clone().unwrap_or_default();
  if !request.tools.is_empty() {
    if !system.is_empty() {
      system.push_str("\n\n");
    }
    system.push_str(&tool_protocol_guidance(&request.tools)?);
  }
  if !system.is_empty() {
    messages.push(PromptMessage::new("system", system));
  }
  for message in &request.messages {
    let role = match message.role {
      Role::System => "system",
      Role::User => "user",
      Role::Assistant => "assistant",
      Role::Tool => "tool",
    };
    let content = if message.role == Role::Assistant && !message.tool_calls.is_empty() {
      message
        .tool_calls
        .iter()
        .map(format_tool_call)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n")
    } else {
      message.content.clone()
    };
    messages.push(PromptMessage::new(role, content));
  }
  Ok(messages)
}

#[cfg(any(feature = "llama-cpp", test))]
pub(super) fn chunk_ranges(len: usize, batch_size: usize) -> Vec<Range<usize>> {
  if batch_size == 0 {
    return Vec::new();
  }
  let mut ranges = Vec::with_capacity(len.div_ceil(batch_size));
  let mut start = 0;
  while start < len {
    let end = (start + batch_size).min(len);
    ranges.push(start..end);
    start = end;
  }
  ranges
}

#[cfg(any(feature = "llama-cpp", test))]
pub(super) async fn acquire_inference_gate(
  gate: Arc<Mutex<()>>,
  sender: mpsc::Sender<Result<ModelEvent, ModelError>>,
) -> Option<OwnedMutexGuard<()>> {
  tokio::select! {
      guard = gate.lock_owned() => Some(guard),
      () = sender.closed() => None,
  }
}

#[cfg(any(feature = "llama-cpp", test))]
pub(super) fn try_admit_inference(
  admission: Arc<Semaphore>,
) -> Result<OwnedSemaphorePermit, ProviderError> {
  admission
    .try_acquire_owned()
    .map_err(|_| ProviderError::Busy)
}

#[cfg(feature = "llama-cpp")]
pub(super) fn render_prompt(
  model: &LlamaModel,
  request: &ModelRequest,
) -> Result<String, ModelError> {
  let template = model.chat_template(None).map_err(|error| {
        ModelError::Other(format!(
            "GGUF model is missing usable tokenizer.chat_template metadata; no fallback template is applied: {error}"
        ))
    })?;
  let mut prompt_messages = prompt_messages(request)?;
  apply_qwen3_thinking_control(
    template.to_str().map_err(|error| {
      ModelError::Other(format!("GGUF chat template is not valid UTF-8: {error}"))
    })?,
    &mut prompt_messages,
    request.thinking,
  );
  let messages = prompt_messages
    .into_iter()
    .map(|message| LlamaChatMessage::new(message.role, message.content))
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| ModelError::Other(format!("invalid chat message: {error}")))?;
  model
    .apply_chat_template(&template, &messages, true)
    .map_err(|error| ModelError::Other(format!("cannot apply GGUF chat template: {error}")))
}

#[cfg(feature = "llama-cpp")]
pub(super) struct StopBuffer<'a> {
  stops: &'a [String],
  pending: String,
  stopped: bool,
}

#[cfg(feature = "llama-cpp")]
impl<'a> StopBuffer<'a> {
  pub(super) fn new(stops: &'a [String]) -> Self {
    Self {
      stops,
      pending: String::new(),
      stopped: false,
    }
  }

  pub(super) fn push(&mut self, piece: &str) -> (Option<String>, bool) {
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
        self
          .stops
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

  pub(super) fn finish(&mut self) -> Option<String> {
    if self.stopped || self.pending.is_empty() {
      None
    } else {
      Some(std::mem::take(&mut self.pending))
    }
  }
}

#[cfg(any(feature = "llama-cpp", test))]
pub(super) fn validate_request(
  config: &LlamaCppConfig,
  request: &ModelRequest,
) -> Result<(), ProviderError> {
  if request.model != config.model.id {
    return Err(ProviderError::InvalidRequest(format!(
      "unknown model id {:?}",
      request.model
    )));
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

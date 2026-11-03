//! The application inference port as a Rig completion model, without a second loop.
use std::sync::Arc;

use agent_inference::{InferenceBackend, ModelInfo};
use rig_core::completion::options::{Mapping, OptionFields, OptionMap};
use rig_core::completion::{Accepts, CompletionRequest, ProviderCapabilities, ReplayTarget};
use rig_core::driver::{Exchange, Opened, Opening, Transport};
use rig_core::error::EncodeError;
use rig_core::message::Api;
use rig_core::operation::Completion;
use rig_core::wire::{Capabilities, Descriptor, Mode, Wire};
use rig_core::{DynModel, Model};

use crate::{ModelConfig, ProviderError, Result};

pub fn local_model(
  config: &ModelConfig,
  backend: Arc<dyn InferenceBackend>,
) -> Result<DynModel<Completion>> {
  crate::configuration::validate(config)?;
  let info = backend.info();
  if config.model != info.id
    || config.context_window > info.context_window
    || config.max_output_tokens >= info.context_window
    || config.supports_images && !info.supports_images
  {
    return Err(ProviderError::Configuration(
      "invalid local model identity, capability or token limit".into(),
    ));
  }
  Ok(
    Model::new(
      LocalWire {
        info,
        max_tokens: config.max_output_tokens,
      },
      LocalTransport(backend),
    )
    .erase(),
  )
}

#[derive(Clone, Debug)]
pub(crate) struct LocalWire {
  pub(crate) info: ModelInfo,
  max_tokens: u32,
}

impl Wire for LocalWire {
  type Op = Completion;
  type Payload = agent_inference::GenerationRequest;
  type Frame = crate::local_stream::Frame;
  type Event = crate::local_stream::Frame;
  type Decoder<'id> = crate::local_stream::Decoder;
  type Reassembler = crate::local_stream::Document;

  fn describe(&self) -> Descriptor<'_> {
    Descriptor::new("lyonbrown-local")
      .model(self.info.id.as_str())
      .capabilities(Capabilities::completion(
        ProviderCapabilities::new().with_forced_tool_choice_rejected(true),
      ))
      .replay(self)
  }

  fn encode(
    &self,
    request: CompletionRequest,
    _mode: Mode,
  ) -> std::result::Result<Self::Payload, EncodeError> {
    crate::local_request::convert(request, &self.info, self.max_tokens)
      .map_err(|_| EncodeError::request("unsupported or invalid local inference request"))
  }

  fn decoder<'id>(&self) -> Self::Decoder<'id> {
    crate::local_stream::Decoder::default()
  }
}

impl ReplayTarget for LocalWire {
  fn api(&self) -> Api {
    Api::from_static("lyonbrown.local")
  }
  fn provider(&self) -> &str {
    "lyonbrown-local"
  }
  fn model(&self) -> &str {
    &self.info.id
  }

  fn catalog_priced(&self) -> bool {
    false
  }

  fn accepts(&self, _model: &str) -> Accepts {
    // Keep tool history for explicit capability validation at the port.
    Accepts {
      user_images: self.info.supports_images,
      tool_result_images: self.info.supports_images,
      ..Accepts::TEXT
    }
  }

  fn map_options(&self, _request: &CompletionRequest, fields: OptionFields<'_>) -> OptionMap {
    let OptionFields {
      reasoning,
      cache,
      service_tier,
      verbosity,
      parallel_tool_calls,
      top_p,
      seed,
      stop,
      cache_key,
    } = fields;
    let unsupported = || Mapping::unsupported("local inference does not support this option");
    OptionMap {
      reasoning: Mapping::of(reasoning, |_| unsupported()),
      cache: Mapping::of(cache, |_| unsupported()),
      service_tier: Mapping::of(service_tier, |_| unsupported()),
      verbosity: Mapping::of(verbosity, |_| unsupported()),
      parallel_tool_calls: Mapping::of(parallel_tool_calls, |_| unsupported()),
      top_p: Mapping::of(top_p, |_| unsupported()),
      seed: Mapping::of(seed, |_| {
        Mapping::Omit("passed through the local inference port")
      }),
      stop: Mapping::of_stop(stop, |_| unsupported()),
      cache_key: Mapping::of(cache_key, |_| unsupported()),
    }
  }
}

#[derive(Clone)]
struct LocalTransport(Arc<dyn InferenceBackend>);

impl Transport<LocalWire> for LocalTransport {
  fn send(
    &self,
    request: agent_inference::GenerationRequest,
    _exchange: Exchange,
  ) -> Opening<crate::local_stream::Frame> {
    let backend = self.0.clone();
    Opening::new(async move { Ok(Opened::new(crate::local_stream::frames(backend, request))) })
  }
}

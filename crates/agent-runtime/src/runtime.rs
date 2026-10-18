use std::sync::Arc;

use rpi_harness::agent_harness::{AgentHarness, AgentLane};
mod result_mapping;
use result_mapping::convert_result;
use rpi_harness::events::{HarnessEvent, OnUnsubscribe, RunEndEvent, RunStartEvent, WatchHandle};
use rpi_harness::runtime::SessionRuntime;
use rpi_harness::session::Session;
use rpi_harness::types::{AgentHarnessOptions, HarnessTool, HarnessToolExecution, RetryPolicy};

use crate::events::Emitter;
use crate::persistence::DurableSession;
use crate::tools::ToolAdapter;
use crate::{
  EventListener, Result, RuntimeConfig, RuntimeError, RuntimeEvent, RuntimeResult, RuntimeTool,
  SessionPersistence,
};

pub struct Runtime {
  harness: AgentHarness,
  _listeners: Vec<OnUnsubscribe>,
  _compaction: WatchHandle<()>,
  execution: tokio::sync::Mutex<()>,
  supports_images: bool,
}

impl Runtime {
  pub async fn open(
    config: RuntimeConfig,
    persistence: Arc<dyn SessionPersistence>,
    tools: Vec<Arc<dyn RuntimeTool>>,
    on_event: EventListener,
  ) -> Result<Self> {
    let (model, provider) = crate::provider::configured(&config)?;
    Self::create(config, persistence, tools, on_event, model, provider).await
  }

  pub async fn open_with_backend(
    config: RuntimeConfig,
    persistence: Arc<dyn SessionPersistence>,
    tools: Vec<Arc<dyn RuntimeTool>>,
    on_event: EventListener,
    backend: Arc<dyn agent_inference::InferenceBackend>,
  ) -> Result<Self> {
    let (model, provider) = crate::local_provider::configured(&config, backend)?;
    Self::create(config, persistence, tools, on_event, model, provider).await
  }

  async fn create(
    config: RuntimeConfig,
    persistence: Arc<dyn SessionPersistence>,
    tools: Vec<Arc<dyn RuntimeTool>>,
    on_event: EventListener,
    model: rpi_ai::Model,
    provider: Arc<dyn rpi_ai::Provider>,
  ) -> Result<Self> {
    let handle = tokio::runtime::Handle::try_current()
      .map_err(|error| RuntimeError::Configuration(error.to_string()))?;
    if handle.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
      return Err(RuntimeError::Configuration(
        "rpi-harness requires a multithreaded Tokio runtime for durable frame callbacks".into(),
      ));
    }
    let storage = DurableSession::open(&config.session_id, config.owner_epoch, persistence).await?;
    let session = Session::new(Arc::new(storage), None);
    ensure_certain(&session).await?;
    let tools: Vec<_> = tools
      .into_iter()
      .map(|tool| HarnessTool::new(Arc::new(ToolAdapter::new(tool))))
      .collect();
    let active_tool_names = tools
      .iter()
      .map(|tool| tool.tool.schema().name.clone())
      .collect();
    let supports_images = model.input.contains(&rpi_ai::InputModality::Image);
    let mut compaction = rpi_harness::types::CompactionSettings::default();
    let context_window = i64::try_from(model.context_window).unwrap_or(i64::MAX);
    compaction.reserve_tokens = compaction.reserve_tokens.min(context_window / 4);
    compaction.keep_recent_tokens = compaction.keep_recent_tokens.min(context_window / 2);
    let harness = AgentHarness::create(AgentHarnessOptions {
      model,
      session,
      models: vec![provider],
      tools,
      active_tool_names,
      system_prompt: config.system_prompt,
      compaction,
      allow_existing_session: true,
      // Retry policy belongs to the durable task scheduler. A failed
      // provider request must not silently create an unbounded retry run.
      retry: RetryPolicy {
        enabled: false,
        ..Default::default()
      },
      tool_execution: HarnessToolExecution::Sequential,
      agent_emitter: Some(Arc::new(Emitter(on_event.clone()))),
      after_tool_call: Some(crate::tools::uncertainty_hook()),
      ..Default::default()
    })
    .await
    .map_err(harness_error)?;
    let started = on_event.clone();
    let ended = on_event.clone();
    let listeners = vec![
      harness.events().on::<RunStartEvent, _>(move |event| {
        started(RuntimeEvent::RunStarted {
          run_id: event.run_id.clone(),
        })
      }),
      harness.events().on::<RunEndEvent, _>(move |event| {
        ended(RuntimeEvent::RunFinished {
          run_id: event.run_id.clone(),
          outcome: event.outcome.as_str().into(),
        })
      }),
    ];
    let mut compaction = harness.events().watch(|| ());
    compaction.start(Arc::new(move |event| {
      if let HarnessEvent::CompactionProgress(event) = event {
        on_event(RuntimeEvent::Compaction {
          active: event.active,
        });
      }
    }));
    Ok(Self {
      harness,
      _listeners: listeners,
      _compaction: compaction,
      execution: tokio::sync::Mutex::new(()),
      supports_images,
    })
  }

  pub async fn execute(&self, prompt: &str) -> Result<RuntimeResult> {
    self.execute_input(prompt, Vec::new()).await
  }

  pub async fn execute_input(
    &self,
    prompt: &str,
    images: Vec<crate::RuntimeImage>,
  ) -> Result<RuntimeResult> {
    if !images.is_empty() && !self.supports_images {
      return Err(RuntimeError::Configuration(
        "configured model does not support image input".into(),
      ));
    }
    crate::input::validate(prompt, &images)?;
    let _execution = self
      .execution
      .try_lock()
      .map_err(|_| RuntimeError::Configuration("session already has an active execution".into()))?;
    ensure_certain(self.harness.session()).await?;
    if self.harness.pending_resume().is_some() {
      return Err(RuntimeError::Configuration(
        "resume the interrupted run before submitting a new prompt".into(),
      ));
    }
    let images = images
      .into_iter()
      .map(|image| rpi_ai::ImageContent {
        kind: Default::default(),
        data: image.data_base64,
        mime_type: image.mime_type,
      })
      .collect();
    let result = self.harness.prompt_text(prompt, images).await;
    ensure_certain(self.harness.session()).await?;
    result.map(convert_result).map_err(harness_error)
  }

  /// Resume only a harness-recorded interrupted run. Never resubmit a prompt.
  pub async fn resume(&self) -> Result<Option<RuntimeResult>> {
    let _execution = self
      .execution
      .try_lock()
      .map_err(|_| RuntimeError::Configuration("session already has an active execution".into()))?;
    ensure_certain(self.harness.session()).await?;
    let result = self.harness.resume_pending().await;
    ensure_certain(self.harness.session()).await?;
    result
      .map(|result| result.map(convert_result))
      .map_err(harness_error)
  }

  pub async fn resume_suspended(&self, id: &str) -> Result<RuntimeResult> {
    let _execution = self
      .execution
      .try_lock()
      .map_err(|_| RuntimeError::Configuration("session already has an active execution".into()))?;
    ensure_certain(self.harness.session()).await?;
    let result = self.harness.resume(id).await;
    ensure_certain(self.harness.session()).await?;
    result.map(convert_result).map_err(harness_error)
  }

  pub async fn cancel(&self) -> Result<()> {
    match self.harness.abort().await {
      Ok(_) | Err(rpi_harness::result::HarnessError::NoActiveRun { .. }) => Ok(()),
      Err(error) => Err(harness_error(error)),
    }
  }
}

fn harness_error(error: impl std::fmt::Display) -> RuntimeError {
  RuntimeError::Harness(error.to_string())
}

async fn ensure_certain(session: &Session) -> Result<()> {
  let entries = session
    .find_entries(&Default::default())
    .await
    .map_err(harness_error)?;
  for entry in entries {
    if let rpi_harness::session::types::Entry::Message(message) = entry
      && let rpi_agent::message::AgentMessage::ToolResult(result) = message.message
      && result
        .details
        .as_ref()
        .is_some_and(|details| details[crate::tools::UNCERTAIN_DETAIL].as_bool() == Some(true))
    {
      return Err(RuntimeError::Uncertain(format!(
        "{} ({}) reported an unknown outcome",
        result.tool_name, result.tool_call_id
      )));
    }
  }
  let report = SessionRuntime::new(session.clone())
    .recover_lane("main")
    .await
    .map_err(harness_error)?;
  if report.is_corrupt() {
    return Err(RuntimeError::Persistence(
      "harness session recovery found corrupt operation records".into(),
    ));
  }
  if !report.tool_frames.is_empty() {
    let tools: Vec<_> = report
      .tool_frames
      .iter()
      .map(|tool| format!("{} ({})", tool.tool_name, tool.tool_call_id))
      .collect();
    return Err(RuntimeError::Uncertain(format!(
      "tool result was not committed: {}",
      tools.join(", ")
    )));
  }
  Ok(())
}

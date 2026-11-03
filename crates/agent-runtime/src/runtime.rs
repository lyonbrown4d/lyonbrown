use crate::{
  EventListener, Result, RuntimeConfig, RuntimeError, RuntimeResult, RuntimeTool,
  SessionPersistence,
  persistence::{ActiveRun, DurableSession},
};
use rig_agent::run::{AgentRun, RunSpec};
use rig_core::{DynModel, operation::Completion};
use std::{
  collections::BTreeMap,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Host lifecycle and IO ownership; model/tool decisions belong only to AgentRun.
pub struct Runtime {
  pub(crate) session: DurableSession,
  pub(crate) privacy: Arc<Mutex<agent_privacy::PrivacyService>>,
  pub(crate) model: DynModel<Completion>,
  pub(crate) config: RuntimeConfig,
  pub(crate) tools: BTreeMap<String, Arc<dyn RuntimeTool>>,
  pub(crate) listener: EventListener,
  pub(crate) execution: Mutex<()>,
  pub(crate) cancellation: Mutex<Option<CancellationToken>>,
  pending_cancel: AtomicBool,
  pub(crate) admission: AtomicBool,
}
impl Runtime {
  pub async fn open(
    config: RuntimeConfig,
    persistence: Arc<dyn SessionPersistence>,
    tools: Vec<Arc<dyn RuntimeTool>>,
    on_event: EventListener,
  ) -> Result<Self> {
    let model = crate::provider::configured(&config)?;
    Self::create(config, persistence, tools, on_event, model).await
  }
  pub async fn open_with_backend(
    config: RuntimeConfig,
    persistence: Arc<dyn SessionPersistence>,
    tools: Vec<Arc<dyn RuntimeTool>>,
    on_event: EventListener,
    backend: Arc<dyn agent_inference::InferenceBackend>,
  ) -> Result<Self> {
    let model = model_provider::local_model(&crate::provider::model_config(&config), backend)
      .map_err(|error| RuntimeError::Configuration(error.to_string()))?;
    Self::create(config, persistence, tools, on_event, model).await
  }
  pub async fn open_with_model(
    config: RuntimeConfig,
    persistence: Arc<dyn SessionPersistence>,
    tools: Vec<Arc<dyn RuntimeTool>>,
    on_event: EventListener,
    model: DynModel<Completion>,
  ) -> Result<Self> {
    Self::create(config, persistence, tools, on_event, model).await
  }
  async fn create(
    mut config: RuntimeConfig,
    persistence: Arc<dyn SessionPersistence>,
    tools: Vec<Arc<dyn RuntimeTool>>,
    listener: EventListener,
    model: DynModel<Completion>,
  ) -> Result<Self> {
    if config.session_id.trim().is_empty() || config.owner_epoch == 0 {
      return Err(RuntimeError::Configuration(
        "session and a positive owner epoch are required".into(),
      ));
    }
    let known = if config.api_key.is_empty() {
      vec![]
    } else {
      vec![secrecy::SecretString::from(config.api_key.clone())]
    };
    let mut privacy = agent_privacy::PrivacyService::new(&known)
      .map_err(|_| RuntimeError::Configuration("privacy initialization failed".into()))?;
    if let Some(saved) = persistence.load_private_scope(&config.session_id).await? {
      privacy
        .import_scope(&config.session_id, &saved)
        .map_err(|_| RuntimeError::Persistence("invalid private scope".into()))?;
    }
    if !config.api_key.is_empty() {
      privacy
        .register_secret(&config.session_id, &config.api_key)
        .map_err(|_| RuntimeError::Configuration("privacy initialization failed".into()))?;
    }
    if let Some(prompt) = &mut config.system_prompt {
      *prompt = privacy
        .protect_text(&config.session_id, prompt)
        .map_err(|_| RuntimeError::Configuration("privacy protection failed".into()))?;
    }
    let saved = privacy
      .export_scope(&config.session_id)
      .map_err(|_| RuntimeError::Persistence("private scope export failed".into()))?;
    persistence
      .commit_private_scope(&config.session_id, config.owner_epoch, saved)
      .await?;
    let session = DurableSession::open(&config.session_id, config.owner_epoch, persistence).await?;
    session.ensure_certain().await?;
    let mut registered = BTreeMap::new();
    for tool in tools {
      let name = tool.definition().name;
      if name.is_empty() || registered.insert(name, tool).is_some() {
        return Err(RuntimeError::Configuration(
          "tool names must be nonempty and unique".into(),
        ));
      }
    }
    let runtime = Self {
      session,
      privacy: Arc::new(Mutex::new(privacy)),
      model,
      config,
      tools: registered,
      listener,
      execution: Mutex::new(()),
      cancellation: Mutex::new(None),
      pending_cancel: AtomicBool::new(false),
      admission: AtomicBool::new(true),
    };
    // Existing snapshots are never silently rewritten: refuse content that cannot
    // satisfy the current boundary before any subsequent commit or model call.
    let snapshot = serde_json::to_value(&*runtime.session.state.lock().await)
      .map_err(|_| RuntimeError::Persistence("session privacy validation failed".into()))?;
    runtime.validate_model_value(&snapshot).await?;
    Ok(runtime)
  }
  /// Install the daemon-owned mapping service before admitting input.
  pub async fn with_privacy(
    mut self,
    shared: Arc<Mutex<agent_privacy::PrivacyService>>,
  ) -> Result<Self> {
    if !Arc::ptr_eq(&self.privacy, &shared) {
      let mut source = self.privacy.lock().await;
      let mut target = shared.lock().await;
      if let Some(saved) = self.session.load_private_scope().await? {
        target
          .import_scope(&self.config.session_id, &saved)
          .map_err(|_| RuntimeError::Persistence("invalid private scope".into()))?;
      }
      source
        .transfer_scope(&mut target, &self.config.session_id)
        .map_err(|_| RuntimeError::Configuration("privacy service transfer failed".into()))?;
      if let Some(prompt) = &mut self.config.system_prompt {
        *prompt = target
          .protect_text(&self.config.session_id, prompt)
          .map_err(|_| RuntimeError::Configuration("privacy protection failed".into()))?;
      }
      let saved = target
        .export_scope(&self.config.session_id)
        .map_err(|_| self.session.private_scope_failed())?;
      self.session.commit_private_scope(saved).await?;
      drop(target);
      drop(source);
      self.privacy = shared;
      let snapshot = serde_json::to_value(&*self.session.state.lock().await)
        .map_err(|_| RuntimeError::Persistence("session privacy validation failed".into()))?;
      self.validate_model_value(&snapshot).await?;
    }
    Ok(self)
  }
  pub async fn execute(&self, prompt: &str) -> Result<RuntimeResult> {
    self.execute_input(prompt, vec![]).await
  }
  pub async fn execute_input(
    &self,
    prompt: &str,
    images: Vec<crate::RuntimeImage>,
  ) -> Result<RuntimeResult> {
    crate::input::validate(prompt, &images)?;
    if !images.is_empty() && !self.config.supports_images {
      return Err(RuntimeError::Configuration(
        "configured model does not support image input".into(),
      ));
    }
    let _execution = self
      .execution
      .try_lock()
      .map_err(|_| RuntimeError::Configuration("session already has an active execution".into()))?;
    self.session.ensure_certain().await?;
    let protected = self.protect_text(prompt).await?;
    let prompt = crate::input::message(&protected, images)?;
    let spec = self.spec();
    self
      .session
      .update("run_started", |state| {
        if state.active.is_some() {
          return Err(RuntimeError::Configuration(
            "resume the existing run before submitting a prompt".into(),
          ));
        }
        let run = AgentRun::from_spec(&spec, prompt, None)
          .with_validated_history(state.history.clone())
          .map_err(crate::persistence::rig_error)?;
        let id = agent_core::new_id();
        state.active = Some(ActiveRun {
          id: id.clone(),
          run,
          spec,
          prepared: None,
          in_flight: None,
          usage: Default::default(),
          segment: 0,
        });
        Ok(((), serde_json::json!({"run_id":id}), vec![]))
      })
      .await?;
    self.run_owned().await
  }
  pub async fn resume(&self) -> Result<Option<RuntimeResult>> {
    let _execution = self
      .execution
      .try_lock()
      .map_err(|_| RuntimeError::Configuration("session already has an active execution".into()))?;
    self.session.ensure_certain().await?;
    if self.session.state.lock().await.active.is_none() {
      return Ok(None);
    }
    self.run_owned().await.map(Some)
  }
  async fn run_owned(&self) -> Result<RuntimeResult> {
    let token = CancellationToken::new();
    {
      let mut current = self.cancellation.lock().await;
      if self.pending_cancel.swap(false, Ordering::AcqRel) {
        token.cancel();
      }
      *current = Some(token.clone());
    }
    self.admission.store(true, Ordering::Release);
    let result = self.drive(&token).await;
    self.close_input_admission().await;
    *self.cancellation.lock().await = None;
    if self.input_persistence_failed() {
      return Err(RuntimeError::Uncertain(
        "session writer is fenced after a persistence failure".into(),
      ));
    }
    result
  }
  pub async fn cancel(&self) -> Result<()> {
    let current = self.cancellation.lock().await;
    if let Some(token) = current.as_ref() {
      token.cancel();
    } else {
      // A daemon can accept cancellation before the execution future is first
      // polled. Keep that decision until the token is installed under this lock.
      self.pending_cancel.store(true, Ordering::Release);
    }
    Ok(())
  }
  pub(crate) fn spec(&self) -> RunSpec {
    RunSpec {
      preamble: self.config.system_prompt.clone(),
      max_tokens: Some(u64::from(self.config.max_output_tokens)),
      max_turns: Some(64),
      max_consecutive_malformed_tool_calls: Some(2),
      ..RunSpec::new()
    }
  }
}

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_context::{CompileRequest, PromptCompiler, TokenEstimator};
use agent_core::{
  AgentError, AgentEvent, ContextItem, ContextItemKind, Locality, ModelTier, ObservationId, StepId,
  StepOutcome, StopReason, TokenUsage, ToolResult, TurnId,
};
use agent_model::{
  ChatMessage, GateVerdict, HeuristicGate, ModelError, ModelEvent, ModelRegistry, ModelRequest,
  ModelScheduler, ModelStream, QualityGate, Role, RoutingPolicy, SchedulerSelection, StepContext,
};
use agent_profile::AgentProfile;
use agent_secret::{NoopBroker, SecretBroker};
use agent_skill::{Skill, SkillRegistry};
use agent_storage::{EventWriter, SessionStore};
use agent_tool::{
  AllowAllApproval, ApprovalDecision, ApprovalPolicy, ToolContext, ToolExecutionError,
  ToolExecutor, ToolRegistry,
};
use agent_workspace::{ProjectType, WorkspaceProfile};
use chrono::Utc;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::{EventBus, KernelState, Session};

#[derive(Debug, Clone)]
pub struct KernelLimits {
  pub max_steps_per_turn: usize,
  pub max_escalations: usize,
  pub tool_timeout: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct KernelModelOptions {
  pub provider: Option<String>,
  pub model: Option<String>,
  pub max_tokens: Option<u32>,
  pub temperature: Option<f32>,
}

impl Default for KernelLimits {
  fn default() -> Self {
    Self {
      max_steps_per_turn: 20,
      max_escalations: 1,
      tool_timeout: Duration::from_secs(60),
    }
  }
}

pub struct Kernel {
  session: Session,
  state: KernelState,
  bus: EventBus,
  scheduler: ModelScheduler,
  tools: ToolRegistry,
  approval: Arc<dyn ApprovalPolicy>,
  broker: Arc<dyn SecretBroker>,
  gate: Arc<dyn QualityGate>,
  limits: KernelLimits,
  cancellation: CancellationToken,
  tool_context: ToolContext,
  model_options: KernelModelOptions,
  agent_profile: AgentProfile,
  workspace_profile: WorkspaceProfile,
  skills: SkillRegistry,
  prompt_compiler: PromptCompiler,
  last_requests: Vec<ModelRequest>,
  storage: Option<SessionStore>,
  writer: Option<EventWriter>,
}

impl Kernel {
  pub fn builder() -> KernelBuilder {
    KernelBuilder::default()
  }

  pub fn state(&self) -> &KernelState {
    &self.state
  }

  pub fn session_id(&self) -> agent_core::SessionId {
    self.session.id
  }

  pub fn session(&self) -> &Session {
    &self.session
  }

  pub async fn close_storage(&mut self) -> Result<(), AgentError> {
    if let Some(writer) = self.writer.take() {
      writer
        .close()
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;
    }
    Ok(())
  }

  pub fn resume(&mut self, events: &[AgentEvent]) -> Result<(), AgentError> {
    if !matches!(self.state, KernelState::Idle) {
      return Err(AgentError::Transition(
        "cannot resume a running kernel".into(),
      ));
    }
    let session = Session::from_events(events)?;
    self.writer = self.storage.as_ref().map(|store| store.writer(session.id));
    self.session = session;
    self.bus = EventBus::with_log(events.to_vec());
    self.last_requests.clear();
    Ok(())
  }

  pub fn event_log(&self) -> &[AgentEvent] {
    self.bus.log()
  }

  pub fn last_requests(&self) -> &[ModelRequest] {
    &self.last_requests
  }

  pub fn cancellation_token(&self) -> CancellationToken {
    self.cancellation.clone()
  }

  pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentEvent> {
    self.bus.subscribe()
  }

  pub async fn run_turn(&mut self, user_text: &str) -> Result<Vec<AgentEvent>, AgentError> {
    let result = self.run_turn_inner(user_text).await;
    if let Some(writer) = &self.writer {
      writer
        .flush()
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;
    }
    result
  }
}

pub(super) struct PreparedStep {
  pub(super) selected: SchedulerSelection,
  pub(super) request: ModelRequest,
  pub(super) step_context: StepContext,
}

mod builder;
mod context;
mod events;
mod fallback;
mod intent;
mod metrics;
mod prepare;
mod tools;
mod turn;

pub use builder::KernelBuilder;
use intent::step_needs_tools;
use metrics::*;

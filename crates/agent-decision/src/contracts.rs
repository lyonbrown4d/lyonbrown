use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Duration};
use tokio_util::sync::CancellationToken;

macro_rules! choices {
  ($(#[$attribute:meta])* $name:ident { $($variant:ident),+ $(,)? }) => {
    $(#[$attribute])*
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum $name { $($variant),+ }
  };
}
choices!(TaskKind {
  Chat,
  Code,
  Browser,
  Desktop,
  Hybrid
});
choices!(ModelPolicy {
  General,
  Reasoning,
  Vision
});
choices!(ExecutionMode {
  Local,
  DistributedAllowed
});
choices!(RiskLevel { Low, Medium, High });
choices!(Capability {
  FileRead,
  FileWrite,
  ProcessExecute,
  Browser,
  Computer,
  Mcp,
  Delegation
});
choices!(
  #[derive(strum::IntoStaticStr)]
  #[strum(serialize_all = "snake_case")]
  Tool {
    ReadFile,
    WriteFile,
    ExecuteCommand,
    BrowserObserve,
    BrowserAct,
    ComputerObserve,
    ComputerAct,
    McpCall,
    DelegateTask
  }
);

impl Tool {
  pub fn capability(self) -> Capability {
    match self {
      Self::ReadFile => Capability::FileRead,
      Self::WriteFile => Capability::FileWrite,
      Self::ExecuteCommand => Capability::ProcessExecute,
      Self::BrowserObserve | Self::BrowserAct => Capability::Browser,
      Self::ComputerObserve | Self::ComputerAct => Capability::Computer,
      Self::McpCall => Capability::Mcp,
      Self::DelegateTask => Capability::Delegation,
    }
  }
  pub fn requires_approval(self) -> bool {
    matches!(
      self,
      Self::WriteFile
        | Self::ExecuteCommand
        | Self::BrowserAct
        | Self::ComputerAct
        | Self::McpCall
        | Self::DelegateTask
    )
  }
  pub fn name(self) -> &'static str {
    self.into()
  }
}

/// Authenticated daemon policy, supplied independently of model output.
#[derive(Clone)]
pub struct DecisionPolicy {
  pub authorized_tools: BTreeSet<Tool>,
  pub available_capabilities: BTreeSet<Capability>,
  pub allowed_models: BTreeSet<ModelPolicy>,
  pub distributed_allowed: bool,
  pub allowed_targets: BTreeSet<String>,
  pub minimum_confidence: f64,
  pub classifier_timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
  Unavailable,
  Timeout,
  InvalidOutput,
  LowConfidence,
  MissingCapability,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionSource {
  Explicit,
  Classifier { restricted: bool },
  Fallback { reason: FallbackReason },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
  pub task_kind: TaskKind,
  pub model_policy: ModelPolicy,
  pub allowed_tools: BTreeSet<Tool>,
  pub required_capabilities: BTreeSet<Capability>,
  pub execution_mode: ExecutionMode,
  pub target_hint: Option<String>,
  pub risk_level: RiskLevel,
  pub confidence: Option<f64>,
  pub trace_id: String,
  pub source: DecisionSource,
}

/// Only the authenticated caller may supply an explicit route. This type is
/// never deserialized from classifier output or inferred from prompt text.
#[derive(Debug, Clone)]
pub struct ExplicitRoute {
  pub task_kind: TaskKind,
  pub model_policy: ModelPolicy,
  pub tools: BTreeSet<Tool>,
  pub execution_mode: ExecutionMode,
  pub target_hint: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecisionError {
  #[error("decision cancelled")]
  Cancelled,
  #[error("invalid decision policy or input")]
  InvalidInput,
  #[error("explicit route exceeds available authority or capabilities")]
  ForbiddenRoute,
}

#[derive(Clone)]
pub struct ClassificationRequest {
  pub system_prompt: String,
  pub user_prompt: String,
}

#[derive(Debug, thiserror::Error)]
#[error("classifier unavailable")]
pub struct ClassifierError;

#[async_trait]
pub trait DecisionBackend: Send + Sync {
  async fn classify(
    &self,
    request: ClassificationRequest,
    cancellation: CancellationToken,
  ) -> Result<String, ClassifierError>;
}

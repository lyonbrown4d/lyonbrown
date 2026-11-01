use crate::config::Config;
use agent_decision::{
  Capability, DecisionEngine, DecisionPolicy, ExecutionMode, ExplicitRoute, ModelPolicy,
  RemoteClassifier, TaskKind, Tool,
};
use std::{sync::Arc, time::Duration};

pub(crate) fn build(
  config: &Config,
  models: fluxdi::Shared<crate::models::Models>,
) -> anyhow::Result<DecisionEngine> {
  config.decision.validate()?;
  let provider = config
    .decision
    .provider
    .clone()
    .unwrap_or_else(|| config.provider.clone());
  let same_provider = provider == config.provider;
  let backend = config.decision.model.as_ref().map(|model| {
    if provider == "local" {
      return Arc::new(RegistryClassifier {
        models: models.clone(),
        id: model.clone(),
      }) as Arc<dyn agent_decision::DecisionBackend>;
    }
    Arc::new(RemoteClassifier::new(agent_runtime::RuntimeConfig {
      session_id: "decision-classifier".into(),
      owner_epoch: 1,
      provider: provider.clone(),
      model: model.clone(),
      api_key: config
        .decision_api_key
        .clone()
        .or_else(|| {
          same_provider
            .then(|| config.provider_api_key.clone())
            .flatten()
        })
        .unwrap_or_default(),
      base_url: config
        .decision
        .api_base
        .clone()
        .or_else(|| same_provider.then(|| config.api_base.clone()).flatten()),
      system_prompt: None,
      context_window: 16_384,
      max_output_tokens: 2048,
      supports_images: false,
    })) as Arc<dyn agent_decision::DecisionBackend>
  });
  let mut authorized_tools = [
    Tool::ReadFile,
    Tool::ListFiles,
    Tool::SearchFiles,
    Tool::WriteFile,
  ]
  .into_iter()
  .collect::<std::collections::BTreeSet<_>>();
  let mut available_capabilities = [Capability::FileRead, Capability::FileWrite]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
  if !config.mcp.is_empty() {
    authorized_tools.insert(Tool::McpCall);
    available_capabilities.insert(Capability::Mcp);
  }
  if config.browser_tools {
    authorized_tools.extend([Tool::BrowserObserve, Tool::BrowserAct]);
    available_capabilities.insert(Capability::Browser);
  }
  if config.computer_tools {
    authorized_tools.extend([Tool::ComputerObserve, Tool::ComputerAct]);
    available_capabilities.insert(Capability::Computer);
  }
  if crate::commands::enabled(config) {
    authorized_tools.insert(Tool::ExecuteCommand);
    available_capabilities.insert(Capability::ProcessExecute);
  }
  let targets = config
    .delegation_targets
    .iter()
    .map(|target| target.node_id.clone())
    .collect::<std::collections::BTreeSet<_>>();
  if !targets.is_empty() {
    authorized_tools.insert(Tool::DelegateTask);
    available_capabilities.insert(Capability::Delegation);
  }
  let reusable = decision_core::DecisionEngine::new(
    config.decision.policy.clone().unwrap_or_default(),
    Vec::new(),
  )?;
  Ok(
    DecisionEngine::new(
      DecisionPolicy {
        authorized_tools,
        available_capabilities,
        allowed_models: [ModelPolicy::General].into(),
        distributed_allowed: !targets.is_empty(),
        allowed_targets: targets,
        minimum_confidence: config.decision.minimum_confidence,
        classifier_timeout: Duration::from_millis(config.decision.timeout_ms),
      },
      backend,
    )?
    .with_reusable(Arc::new(reusable)),
  )
}

pub(crate) fn workspace_rule(
  mcp: bool,
  browser: bool,
  commands: bool,
  delegation: bool,
  computer: bool,
) -> ExplicitRoute {
  let mut tools = [
    Tool::ReadFile,
    Tool::ListFiles,
    Tool::SearchFiles,
    Tool::WriteFile,
  ]
  .into_iter()
  .collect::<std::collections::BTreeSet<_>>();
  if mcp {
    tools.insert(Tool::McpCall);
  }
  if browser {
    tools.extend([Tool::BrowserObserve, Tool::BrowserAct]);
  }
  if commands {
    tools.insert(Tool::ExecuteCommand);
  }
  if delegation {
    tools.insert(Tool::DelegateTask);
  }
  if computer {
    tools.extend([Tool::ComputerObserve, Tool::ComputerAct]);
  }
  ExplicitRoute {
    task_kind: TaskKind::Code,
    model_policy: ModelPolicy::General,
    tools,
    execution_mode: if delegation {
      ExecutionMode::DistributedAllowed
    } else {
      ExecutionMode::Local
    },
    target_hint: None,
  }
}

struct RegistryClassifier {
  models: fluxdi::Shared<crate::models::Models>,
  id: String,
}
#[async_trait::async_trait]
impl agent_decision::DecisionBackend for RegistryClassifier {
  async fn classify(
    &self,
    request: agent_decision::ClassificationRequest,
    cancellation: tokio_util::sync::CancellationToken,
  ) -> Result<String, agent_decision::ClassifierError> {
    let backend = self
      .models
      .backend(&self.id)
      .await
      .map_err(|_| agent_decision::ClassifierError)?;
    agent_decision::DecisionBackend::classify(
      &agent_decision::LocalClassifier::new(backend),
      request,
      cancellation,
    )
    .await
  }
}

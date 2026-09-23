use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_kernel::{Kernel, KernelModelOptions};
use agent_model::{ModelRegistry, RoutingPolicy, TaskRouter};
use agent_profile::AgentProfile;
use agent_secret::SecretBroker;
use agent_skill::SkillRegistry;
use agent_storage::SessionStore;
use agent_tool::{ApprovalPolicy, ToolRegistry};
use agent_workspace::WorkspaceProfile;
use anyhow::{Context, Result};
use fluxdi::{Injector, Module, Provider, Shared};

use crate::config::RuntimeConfig;

use super::build::{PreparedServices, prepare_services};

#[derive(Debug, Clone)]
pub struct RuntimePaths {
  workspace: PathBuf,
  home: PathBuf,
}

impl RuntimePaths {
  pub fn new(workspace: PathBuf, home: PathBuf) -> Self {
    Self { workspace, home }
  }

  pub fn workspace(&self) -> &Path {
    &self.workspace
  }

  pub fn home(&self) -> &Path {
    &self.home
  }
}

struct OptionalTaskRouter(Option<Arc<dyn TaskRouter>>);
struct OptionalStorage(Option<SessionStore>);

struct KernelFactory {
  config: Shared<RuntimeConfig>,
  paths: Shared<RuntimePaths>,
  registry: Shared<ModelRegistry>,
  task_router: Shared<OptionalTaskRouter>,
  tools: Shared<ToolRegistry>,
  approval: Shared<dyn ApprovalPolicy>,
  broker: Shared<dyn SecretBroker>,
  profile: Shared<AgentProfile>,
  workspace_profile: Shared<WorkspaceProfile>,
  skills: Shared<SkillRegistry>,
  storage: Shared<OptionalStorage>,
}

impl KernelFactory {
  fn build(&self) -> Kernel {
    let routing_policy = RoutingPolicy {
      prefer_local: self.profile.model.prefer_local,
      remote_escalation: self.profile.model.remote_escalation,
      ..RoutingPolicy::default()
    };
    let mut builder = Kernel::builder()
      .providers(self.registry.as_ref().clone())
      .routing_policy(routing_policy)
      .tools(self.tools.as_ref().clone())
      .approval_arc(self.approval.clone())
      .broker_arc(self.broker.clone())
      .workspace_root(self.paths.workspace())
      .agent_profile(self.profile.as_ref().clone())
      .workspace_profile(self.workspace_profile.as_ref().clone())
      .skills(self.skills.as_ref().clone())
      .model_options(KernelModelOptions {
        provider: Some(self.config.model.provider.clone()),
        model: self.config.model.model.clone(),
        max_tokens: Some(self.config.model.max_tokens),
        temperature: self.config.model.temperature,
        thinking: self.config.model.thinking,
      });
    if let Some(router) = &self.task_router.0 {
      builder = builder.task_router(router.clone());
    }
    if let Some(storage) = &self.storage.0 {
      builder = builder.storage(storage.clone());
    }
    builder.build()
  }
}

struct CoreModule {
  config: Shared<RuntimeConfig>,
  paths: Shared<RuntimePaths>,
  approval: Shared<dyn ApprovalPolicy>,
}

impl Module for CoreModule {
  fn providers(&self, injector: &Injector) {
    let config = self.config.clone();
    injector.provide::<RuntimeConfig>(Provider::root(move |_| config.clone()));
    let paths = self.paths.clone();
    injector.provide::<RuntimePaths>(Provider::root(move |_| paths.clone()));
    let approval = self.approval.clone();
    injector.provide::<dyn ApprovalPolicy>(Provider::root(move |_| approval.clone()));
  }
}

struct RuntimeModule {
  services: PreparedServices,
}

impl Module for RuntimeModule {
  fn providers(&self, injector: &Injector) {
    register_value(injector, self.services.registry.clone());
    register_value(injector, self.services.tools.clone());
    register_value(injector, self.services.profile.clone());
    register_value(injector, self.services.workspace_profile.clone());
    register_value(injector, self.services.skills.clone());
    register_value(
      injector,
      OptionalTaskRouter(self.services.task_router.clone()),
    );
    register_value(injector, OptionalStorage(self.services.storage.clone()));
    let broker = self.services.broker.clone();
    injector.provide::<dyn SecretBroker>(Provider::root(move |_| broker.clone()));
    injector.provide::<KernelFactory>(
      Provider::root(|inj| {
        Shared::new(KernelFactory {
          config: inj.resolve(),
          paths: inj.resolve(),
          registry: inj.resolve(),
          task_router: inj.resolve(),
          tools: inj.resolve(),
          approval: inj.resolve(),
          broker: inj.resolve(),
          profile: inj.resolve(),
          workspace_profile: inj.resolve(),
          skills: inj.resolve(),
          storage: inj.resolve(),
        })
      })
      .with_dependency::<RuntimeConfig>()
      .with_dependency::<RuntimePaths>()
      .with_dependency::<ModelRegistry>()
      .with_dependency::<OptionalTaskRouter>()
      .with_dependency::<ToolRegistry>()
      .with_dependency::<dyn ApprovalPolicy>()
      .with_dependency::<dyn SecretBroker>()
      .with_dependency::<AgentProfile>()
      .with_dependency::<WorkspaceProfile>()
      .with_dependency::<SkillRegistry>()
      .with_dependency::<OptionalStorage>(),
    );
  }
}

fn register_value<T>(injector: &Injector, value: T)
where
  T: Send + Sync + 'static,
{
  let value = Shared::new(value);
  injector.provide::<T>(Provider::root(move |_| value.clone()));
}

pub struct CompositionRoot {
  injector: Injector,
}

impl CompositionRoot {
  pub fn new(
    config: RuntimeConfig,
    paths: RuntimePaths,
    approval: Arc<dyn ApprovalPolicy>,
  ) -> Result<Self> {
    let injector = Injector::root();
    CoreModule {
      config: Shared::new(config),
      paths: Shared::new(paths),
      approval,
    }
    .configure(&injector)
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(Self { injector })
  }

  pub async fn bootstrap(
    config: RuntimeConfig,
    paths: RuntimePaths,
    approval: Arc<dyn ApprovalPolicy>,
  ) -> Result<Self> {
    let root = Self::new(config, paths, approval)?;
    let config = root.resolve::<RuntimeConfig>()?;
    let paths = root.resolve::<RuntimePaths>()?;
    let services = prepare_services(&config, paths.workspace(), paths.home()).await?;
    RuntimeModule { services }
      .configure(&root.injector)
      .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    root.validate()?;
    Ok(root)
  }

  fn resolve<T>(&self) -> Result<Shared<T>>
  where
    T: ?Sized + Send + Sync + 'static,
  {
    self
      .injector
      .try_resolve::<T>()
      .map_err(|error| anyhow::anyhow!(error.to_string()))
  }

  pub fn config(&self) -> Result<Shared<RuntimeConfig>> {
    self.resolve()
  }

  pub fn paths(&self) -> Result<Shared<RuntimePaths>> {
    self.resolve()
  }

  pub fn validate(&self) -> Result<()> {
    self
      .injector
      .try_validate_graph()
      .map_err(|error| anyhow::anyhow!(error.to_string()))
      .context("FluxDI dependency graph validation failed")
  }

  pub fn build_kernel(&self) -> Result<Kernel> {
    Ok(self.resolve::<KernelFactory>()?.build())
  }
}

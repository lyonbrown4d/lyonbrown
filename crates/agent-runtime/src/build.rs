use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_mcp::McpBridge;
#[cfg(feature = "llama-cpp")]
use agent_model::ModelTaskRouter;
use agent_model::{ModelRegistry, TaskRouter};
use agent_profile::{AgentProfile, ProfileError, ProfileLoader};
use agent_secret::{CompositeBroker, SecretBroker};
use agent_skill::SkillRegistry;
use agent_storage::SessionStore;
use agent_tool::{
  ListFilesTool, ReadFileTool, SearchWorkspaceTool, ShellTool, ToolRegistry, WriteFileTool,
};
use agent_workspace::WorkspaceProfile;
use anyhow::Result;
use provider_anthropic::AnthropicProvider;
#[cfg(feature = "llama-cpp")]
use provider_llamacpp::{GgufModel, LlamaCppConfig, LlamaCppProvider};
use provider_ollama::OllamaProvider;
use provider_openai::OpenAiProvider;

use crate::config::RuntimeConfig;

pub fn storage_path(config: &RuntimeConfig, workspace: &Path, home: &Path) -> PathBuf {
  match &config.storage.path {
    Some(path) if path.is_absolute() => path.clone(),
    Some(path) => workspace.join(path),
    None => home.join(".agent").join("agent.db"),
  }
}

pub struct PreparedServices {
  pub registry: ModelRegistry,
  pub task_router: Option<Arc<dyn TaskRouter>>,
  pub broker: Arc<dyn SecretBroker>,
  pub tools: ToolRegistry,
  pub workspace_profile: WorkspaceProfile,
  pub profile: AgentProfile,
  pub skills: SkillRegistry,
  pub storage: Option<SessionStore>,
}

pub async fn prepare_services(
  config: &RuntimeConfig,
  workspace: &Path,
  home: &Path,
) -> Result<PreparedServices> {
  let registry = ModelRegistry::new();
  registry.register(Arc::new(OllamaProvider::with_default_models(
    &config.ollama.endpoint,
  )?));
  let task_router = register_llamacpp(&registry, config, workspace, home).await?;
  let broker: Arc<dyn SecretBroker> = Arc::new(CompositeBroker::for_home(home)?);
  if config.anthropic.enabled || config.model.provider == "anthropic" {
    registry.register(Arc::new(AnthropicProvider::with_secret_ref_default_models(
      broker.clone(),
      config.anthropic.api_key_ref.clone(),
      &config.anthropic.endpoint,
    )?));
  }
  if config.openai.enabled || config.model.provider == "openai" {
    registry.register(Arc::new(OpenAiProvider::with_secret_ref_default_models(
      broker.clone(),
      config.openai.api_key_ref.clone(),
      &config.openai.endpoint,
    )?));
  }
  let tools = ToolRegistry::new();
  tools.register(Arc::new(ReadFileTool::new()));
  tools.register(Arc::new(WriteFileTool::new()));
  tools.register(Arc::new(ListFilesTool::new()));
  tools.register(Arc::new(ShellTool::new()));
  let index_path = agent_workspace::workspace_cache_path(home, workspace)?.join("index.db");
  let index_root = workspace.to_path_buf();
  let (index, watcher) = tokio::task::spawn_blocking(move || {
    agent_index::WorkspaceIndex::open_and_watch(index_root, index_path)
  })
  .await??;
  tools.register(Arc::new(SearchWorkspaceTool::new(index, watcher)));
  for (name, server) in &config.mcp.servers {
    McpBridge::connect(name, server).await?.register(&tools);
  }

  let workspace_profile = WorkspaceProfile::detect(workspace)?;
  let profile = load_agent_profile(home, &config.profile)?;
  let skills = SkillRegistry::load_from_root(home.join(".agent").join("skills"))?;
  let storage = if config.storage.enabled {
    Some(SessionStore::open(storage_path(config, workspace, home)).await?)
  } else {
    None
  };
  Ok(PreparedServices {
    registry,
    task_router,
    broker,
    tools,
    workspace_profile,
    profile,
    skills,
    storage,
  })
}

#[cfg(feature = "llama-cpp")]
async fn register_llamacpp(
  registry: &ModelRegistry,
  config: &RuntimeConfig,
  workspace: &Path,
  home: &Path,
) -> Result<Option<Arc<dyn TaskRouter>>> {
  if config.model.provider != "llamacpp" {
    return Ok(None);
  }
  let (automatic_id, model_path) = if let Some(configured_path) = &config.llamacpp.model_path {
    let path = if configured_path.is_absolute() {
      configured_path.clone()
    } else {
      workspace.join(configured_path)
    };
    (None, path)
  } else {
    let prepared = crate::model_manager::resolve_cached_model(home, config.model.model.as_deref())
      .await?
      .ok_or_else(|| {
        anyhow::anyhow!(
          "local model is not installed; configure a model path or choose another provider"
        )
      })?;
    (Some(prepared.id), prepared.path)
  };
  let model_id = config
    .model
    .model
    .as_deref()
    .or(automatic_id)
    .unwrap_or("local");
  let model = GgufModel::new(
    model_id,
    model_path,
    config.llamacpp.context_size,
    config.llamacpp.max_output_tokens,
  )?;
  let provider = Arc::new(LlamaCppProvider::new(LlamaCppConfig {
    model,
    batch_size: config.llamacpp.batch_size,
    threads: config.llamacpp.threads,
    batch_threads: config.llamacpp.batch_threads,
    gpu_layers: config.llamacpp.gpu_layers,
    temperature: config.llamacpp.temperature,
    seed: config.llamacpp.seed,
  })?);
  registry.register(provider.clone());
  Ok(Some(Arc::new(ModelTaskRouter::new(provider, model_id))))
}

fn load_agent_profile(home: &Path, id: &str) -> Result<AgentProfile> {
  match ProfileLoader::new(home).load(id) {
    Ok(profile) => Ok(profile),
    Err(ProfileError::Io { source, .. })
      if id == "default-dev" && source.kind() == io::ErrorKind::NotFound =>
    {
      Ok(AgentProfile::builtin_default())
    }
    Err(error) => Err(error.into()),
  }
}

#[cfg(not(feature = "llama-cpp"))]
async fn register_llamacpp(
  _registry: &ModelRegistry,
  config: &RuntimeConfig,
  _workspace: &Path,
  _home: &Path,
) -> Result<Option<Arc<dyn TaskRouter>>> {
  if config.model.provider == "llamacpp" {
    anyhow::bail!(
      "embedded llama.cpp support is not included in this build; enable the llama-cpp feature"
    );
  }
  Ok(None)
}

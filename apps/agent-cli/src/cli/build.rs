use super::*;

pub(super) fn storage_path(config: &RuntimeConfig, workspace: &Path, home: &Path) -> PathBuf {
  match &config.storage.path {
    Some(path) if path.is_absolute() => path.clone(),
    Some(path) => workspace.join(path),
    None => home.join(".agent").join("agent.db"),
  }
}

pub(super) async fn build_kernel(
  config: &RuntimeConfig,
  workspace: &Path,
  home: &Path,
  approval: SessionApprovalPolicy,
) -> Result<Kernel> {
  let registry = ModelRegistry::new();
  registry.register(Arc::new(OllamaProvider::with_default_models(
    &config.ollama.endpoint,
  )?));
  register_llamacpp(&registry, config, workspace)?;
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
  let routing_policy = RoutingPolicy {
    prefer_local: profile.model.prefer_local,
    remote_escalation: profile.model.remote_escalation,
    ..RoutingPolicy::default()
  };

  let mut builder = Kernel::builder()
    .providers(registry)
    .routing_policy(routing_policy)
    .tools(tools)
    .approval(approval)
    .broker_arc(broker)
    .workspace_root(workspace)
    .agent_profile(profile)
    .workspace_profile(workspace_profile)
    .skills(skills)
    .model_options(KernelModelOptions {
      provider: Some(config.model.provider.clone()),
      model: config.model.model.clone(),
      max_tokens: Some(config.model.max_tokens),
      temperature: config.model.temperature,
    });
  if config.storage.enabled {
    builder = builder.storage(SessionStore::open(storage_path(config, workspace, home)).await?);
  }
  Ok(builder.build())
}

#[cfg(feature = "llama-cpp")]
fn register_llamacpp(
  registry: &ModelRegistry,
  config: &RuntimeConfig,
  workspace: &Path,
) -> Result<()> {
  if config.model.provider != "llamacpp" {
    return Ok(());
  }
  let configured_path = config
    .llamacpp
    .model_path
    .as_ref()
    .context("llamacpp.model_path is required")?;
  let model_path = if configured_path.is_absolute() {
    configured_path.clone()
  } else {
    workspace.join(configured_path)
  };
  let model_id = config.model.model.as_deref().unwrap_or("local");
  let model = GgufModel::new(
    model_id,
    model_path,
    config.llamacpp.context_size,
    config.llamacpp.max_output_tokens,
  )?;
  let provider = LlamaCppProvider::new(LlamaCppConfig {
    model,
    batch_size: config.llamacpp.batch_size,
    threads: config.llamacpp.threads,
    batch_threads: config.llamacpp.batch_threads,
    gpu_layers: config.llamacpp.gpu_layers,
    temperature: config.llamacpp.temperature,
    seed: config.llamacpp.seed,
  })?;
  registry.register(Arc::new(provider));
  Ok(())
}

#[cfg(not(feature = "llama-cpp"))]
fn register_llamacpp(
  _registry: &ModelRegistry,
  config: &RuntimeConfig,
  _workspace: &Path,
) -> Result<()> {
  if config.model.provider == "llamacpp" {
    anyhow::bail!(
      "embedded llama.cpp support is not included in this build; rebuild agent-cli with --features llama-cpp"
    );
  }
  Ok(())
}

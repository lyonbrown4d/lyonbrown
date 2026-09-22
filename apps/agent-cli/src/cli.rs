use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_core::{AgentError, AgentEvent};
use agent_kernel::{Kernel, KernelModelOptions};
use agent_model::{ModelRegistry, RoutingPolicy};
use agent_profile::{AgentProfile, ProfileError, ProfileLoader};
use agent_skill::SkillRegistry;
use agent_tool::{
    ListFilesTool, ReadFileTool, SessionApprovalPolicy, ShellTool, ToolRegistry, WriteFileTool,
};
use agent_workspace::WorkspaceProfile;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use provider_ollama::OllamaProvider;

use crate::approval::policy_from_config;
use crate::config::{CliOverrides, LoadOptions, RuntimeConfig};
use crate::tui::TuiApprovalPrompt;

#[derive(Debug, Parser)]
#[command(name = "agent", version, about = "Local-first agent runtime")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Explicit configuration file (TOML, YAML, JSON, JSON5, RON, or INI).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[arg(long, global = true)]
    pub provider: Option<String>,

    #[arg(long, global = true)]
    pub model: Option<String>,

    /// Agent profile ID from ~/.agent/profiles.
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Ollama base URL.
    #[arg(long, global = true)]
    pub endpoint: Option<String>,

    /// Allow all tool requests for this session.
    #[arg(long, global = true, conflicts_with = "deny_all")]
    pub approve_all: bool,

    /// Deny all write, execute, and network tool requests for this session.
    #[arg(long, global = true, conflicts_with = "approve_all")]
    pub deny_all: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Ask one question and stream the answer.
    Ask { prompt: String },
    /// Start a multi-turn terminal chat.
    Chat,
    /// Open the workspace terminal interface.
    #[command(name = ".")]
    Tui {
        #[arg(skip = PathBuf::from("."))]
        workspace: PathBuf,
    },
}

pub async fn run(cli: Cli) -> Result<()> {
    let workspace = match &cli.command {
        Command::Tui { workspace } => workspace.clone(),
        _ => std::env::current_dir().context("failed to determine workspace directory")?,
    };
    let mut options = LoadOptions::for_workspace(&workspace);
    options.explicit_config = cli.config;
    options.cli = CliOverrides {
        provider: cli.provider,
        model: cli.model,
        profile: cli.profile,
        endpoint: cli.endpoint,
        approve_all: cli.approve_all.then_some(true),
        deny_all: cli.deny_all.then_some(true),
        ..Default::default()
    };
    let config = RuntimeConfig::load(options)?;
    let home = dirs::home_dir().context("home directory is unavailable")?;
    let (approval, tui_approvals) = match &cli.command {
        Command::Tui { .. } if !config.approval.approve_all && !config.approval.deny_all => {
            let (prompt, requests) = TuiApprovalPrompt::channel();
            (
                SessionApprovalPolicy::interactive(Arc::new(prompt)),
                Some(requests),
            )
        }
        _ => (policy_from_config(&config.approval), None),
    };
    let mut kernel = build_kernel(&config, &workspace, &home, approval)?;

    match cli.command {
        Command::Ask { prompt } => ask(&mut kernel, &prompt).await,
        Command::Chat => chat(&mut kernel).await,
        Command::Tui { .. } => {
            let requests = tui_approvals.unwrap_or_else(|| {
                let (_sender, receiver) = std::sync::mpsc::channel();
                receiver
            });
            crate::tui::run(kernel, requests).await
        }
    }
}

fn build_kernel(
    config: &RuntimeConfig,
    workspace: &Path,
    home: &Path,
    approval: SessionApprovalPolicy,
) -> Result<Kernel> {
    let registry = ModelRegistry::new();
    registry.register(Arc::new(OllamaProvider::with_default_models(
        &config.ollama.endpoint,
    )?));
    let tools = ToolRegistry::new();
    tools.register(Arc::new(ReadFileTool::new()));
    tools.register(Arc::new(WriteFileTool::new()));
    tools.register(Arc::new(ListFilesTool::new()));
    tools.register(Arc::new(ShellTool::new()));

    let workspace_profile = WorkspaceProfile::detect(workspace)?;
    let profile = load_agent_profile(home, &config.profile)?;
    let skills = SkillRegistry::load_from_root(home.join(".agent").join("skills"))?;
    let routing_policy = RoutingPolicy {
        prefer_local: profile.model.prefer_local,
        remote_escalation: profile.model.remote_escalation,
        ..RoutingPolicy::default()
    };

    Ok(Kernel::builder()
        .providers(registry)
        .routing_policy(routing_policy)
        .tools(tools)
        .approval(approval)
        .workspace_root(workspace)
        .agent_profile(profile)
        .workspace_profile(workspace_profile)
        .skills(skills)
        .model_options(KernelModelOptions {
            provider: Some(config.model.provider.clone()),
            model: config.model.model.clone(),
            max_tokens: Some(config.model.max_tokens),
            temperature: config.model.temperature,
        })
        .build())
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

async fn ask(kernel: &mut Kernel, prompt: &str) -> Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    run_kernel_turn(kernel, prompt, &mut output).await?;
    writeln!(output)?;
    Ok(())
}

async fn chat(kernel: &mut Kernel) -> Result<()> {
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut prompt = String::new();
        if io::stdin().read_line(&mut prompt)? == 0 {
            break;
        }
        let prompt = prompt.trim();
        if prompt.is_empty() {
            continue;
        }
        if matches!(prompt, "/exit" | "/quit") {
            break;
        }
        let stdout = io::stdout();
        let mut output = stdout.lock();
        run_kernel_turn(kernel, prompt, &mut output).await?;
        writeln!(output)?;
    }
    Ok(())
}

pub async fn run_kernel_turn(
    kernel: &mut Kernel,
    prompt: &str,
    output: &mut impl Write,
) -> Result<Vec<AgentEvent>, AgentError> {
    let mut events = kernel.subscribe();
    let turn = kernel.run_turn(prompt);
    tokio::pin!(turn);
    loop {
        tokio::select! {
            biased;
            event = events.recv() => {
                match event {
                    Ok(event) => render_event(&event, output)?,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        return Err(AgentError::Tool(format!(
                            "CLI event renderer lagged by {skipped} events"
                        )));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err(AgentError::Tool("kernel event stream closed".into()));
                    }
                }
            }
            result = &mut turn => {
                let result = result?;
                loop {
                    match events.try_recv() {
                        Ok(event) => render_event(&event, output)?,
                        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                        Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                            return Err(AgentError::Tool(format!(
                                "CLI event renderer lagged by {skipped} events"
                            )));
                        }
                    }
                }
                return Ok(result);
            },
        }
    }
}

fn render_event(event: &AgentEvent, output: &mut impl Write) -> Result<(), AgentError> {
    if let AgentEvent::ModelDelta { delta, .. } = event {
        output
            .write_all(delta.as_bytes())
            .map_err(|error| AgentError::Tool(error.to_string()))?;
        output
            .flush()
            .map_err(|error| AgentError::Tool(error.to_string()))?;
    }
    Ok(())
}

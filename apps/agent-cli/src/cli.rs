use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_core::{AgentError, AgentEvent};
use agent_kernel::{Kernel, KernelModelOptions};
use agent_model::ModelRegistry;
use agent_tool::{ListFilesTool, ReadFileTool, ShellTool, ToolRegistry, WriteFileTool};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use provider_ollama::OllamaProvider;

use crate::approval::policy_from_config;
use crate::config::{CliOverrides, LoadOptions, RuntimeConfig};

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
}

pub async fn run(cli: Cli) -> Result<()> {
    let workspace = std::env::current_dir().context("failed to determine workspace directory")?;
    let mut options = LoadOptions::for_workspace(&workspace);
    options.explicit_config = cli.config;
    options.cli = CliOverrides {
        provider: cli.provider,
        model: cli.model,
        endpoint: cli.endpoint,
        approve_all: cli.approve_all.then_some(true),
        deny_all: cli.deny_all.then_some(true),
        ..Default::default()
    };
    let config = RuntimeConfig::load(options)?;
    let mut kernel = build_kernel(&config, &workspace)?;

    match cli.command {
        Command::Ask { prompt } => ask(&mut kernel, &prompt).await,
        Command::Chat => chat(&mut kernel).await,
    }
}

fn build_kernel(config: &RuntimeConfig, workspace: &Path) -> Result<Kernel> {
    let registry = ModelRegistry::new();
    registry.register(Arc::new(OllamaProvider::with_default_models(
        &config.ollama.endpoint,
    )?));
    let tools = ToolRegistry::new();
    tools.register(Arc::new(ReadFileTool::new()));
    tools.register(Arc::new(WriteFileTool::new()));
    tools.register(Arc::new(ListFilesTool::new()));
    tools.register(Arc::new(ShellTool::new()));

    Ok(Kernel::builder()
        .providers(registry)
        .tools(tools)
        .approval(policy_from_config(&config.approval))
        .workspace_root(workspace)
        .model_options(KernelModelOptions {
            provider: Some(config.model.provider.clone()),
            model: config.model.model.clone(),
            max_tokens: Some(config.model.max_tokens),
            temperature: config.model.temperature,
        })
        .build())
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
